use super::{
    executor::OperationExecutor,
    model::{ExecutionRecord, ExecutionState, ExecutionTransitionError, OperationDefinition},
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{Mutex, Semaphore, oneshot, watch},
    task::JoinHandle,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionManagerError {
    InvalidCapacity,
    DuplicateOperation(String),
    UnknownOperation(String),
    ExecutionNotSupported(String),
    UnknownExecution(String),
    UnsupportedCapability(&'static str),
    CapacityFull,
    InvalidTransition(ExecutionTransitionError),
    CapabilityMismatch(String),
    DuplicateExecutor(String),
    ExecutorNotRegistered(String),
    ConcurrencyLimitReached,
    InvalidTimeout,
    ManagerShuttingDown,
}

struct ManagerState {
    operations: HashMap<String, OperationDefinition>,
    executions: HashMap<String, ExecutionRecord>,
    execution_order: VecDeque<String>,
}

/// Shared execution registry and lifecycle manager.
///
/// Active records are never evicted to make room for new executions.
#[derive(Clone)]
pub struct ExecutionManager {
    state: Arc<Mutex<ManagerState>>,
    executors: Arc<Mutex<HashMap<String, Arc<dyn OperationExecutor>>>>,
    tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    stop_signals: Arc<Mutex<HashMap<String, watch::Sender<bool>>>>,
    concurrency: Arc<Semaphore>,
    next_execution_id: Arc<AtomicU64>,
    capacity: usize,
    lifecycle: Arc<Mutex<bool>>,
}

impl ExecutionManager {
    pub fn new(capacity: usize) -> Result<Self, ExecutionManagerError> {
        Self::new_with_limits(capacity, capacity)
    }

    /// Creates a manager with independent history and concurrency limits.
    pub fn new_with_limits(
        capacity: usize,
        max_concurrent: usize,
    ) -> Result<Self, ExecutionManagerError> {
        if capacity == 0 || max_concurrent == 0 {
            return Err(ExecutionManagerError::InvalidCapacity);
        }

        Ok(Self {
            state: Arc::new(Mutex::new(ManagerState {
                operations: HashMap::new(),
                executions: HashMap::new(),
                execution_order: VecDeque::with_capacity(capacity),
            })),
            executors: Arc::new(Mutex::new(HashMap::new())),
            tasks: Arc::new(Mutex::new(HashMap::new())),
            stop_signals: Arc::new(Mutex::new(HashMap::new())),
            concurrency: Arc::new(Semaphore::new(max_concurrent)),
            next_execution_id: Arc::new(AtomicU64::new(1)),
            capacity,
            lifecycle: Arc::new(Mutex::new(false)),
        })
    }

    pub async fn register_operation(
        &self,
        operation: OperationDefinition,
    ) -> Result<(), ExecutionManagerError> {
        let mut state = self.state.lock().await;

        if state.operations.contains_key(&operation.id) {
            return Err(ExecutionManagerError::DuplicateOperation(operation.id));
        }

        state.operations.insert(operation.id.clone(), operation);
        Ok(())
    }

    /// Registers the executor implementation for an existing operation.
    ///
    /// The executor's capabilities must exactly match the operation metadata.
    pub async fn register_executor(
        &self,
        operation_id: &str,
        executor: Arc<dyn OperationExecutor>,
    ) -> Result<(), ExecutionManagerError> {
        let operation = self.get_operation(operation_id).await?;
        let executor_capabilities = executor.capabilities();

        if operation.capabilities != executor_capabilities {
            return Err(ExecutionManagerError::CapabilityMismatch(
                operation_id.to_string(),
            ));
        }

        let mut executors = self.executors.lock().await;

        if executors.contains_key(operation_id) {
            return Err(ExecutionManagerError::DuplicateExecutor(
                operation_id.to_string(),
            ));
        }

        executors.insert(operation_id.to_string(), executor);
        Ok(())
    }

    /// Returns a cloned executor handle without retaining the registry lock.
    pub async fn get_executor(
        &self,
        operation_id: &str,
    ) -> Result<Arc<dyn OperationExecutor>, ExecutionManagerError> {
        let executors = self.executors.lock().await;

        executors
            .get(operation_id)
            .cloned()
            .ok_or_else(|| ExecutionManagerError::ExecutorNotRegistered(operation_id.to_string()))
    }

    /// Starts a registered executor without waiting for a concurrency slot.
    ///
    /// Active work is rejected immediately when the concurrency limit is full.
    pub async fn start_execution(
        &self,
        ecu_id: String,
        operation_id: &str,
        timeout: Option<Duration>,
    ) -> Result<ExecutionRecord, ExecutionManagerError> {
        if timeout.is_some_and(|duration| duration.is_zero()) {
            return Err(ExecutionManagerError::InvalidTimeout);
        }

        // Hold admission lock until task registration is complete. Shutdown
        // cannot miss a task that is between admission and registration.
        let lifecycle_guard = self.lifecycle.lock().await;
        if *lifecycle_guard {
            return Err(ExecutionManagerError::ManagerShuttingDown);
        }

        let executor = self.get_executor(operation_id).await?;

        let permit = self
            .concurrency
            .clone()
            .try_acquire_owned()
            .map_err(|_| ExecutionManagerError::ConcurrencyLimitReached)?;

        let created = self
            .create_execution(ecu_id.clone(), operation_id, current_time_ms())
            .await?;

        let running = self
            .transition_execution(
                &created.execution_id,
                ExecutionState::Running,
                current_time_ms(),
            )
            .await?;

        let (stop_sender, stop_receiver) = watch::channel(false);
        let stop_observer = stop_receiver.clone();
        let (start_sender, start_receiver) = oneshot::channel();

        let execution_id = running.execution_id.clone();
        let task_execution_id = execution_id.clone();
        let task_manager = self.clone();
        let task_operation_id = operation_id.to_string();

        let task = tokio::spawn(async move {
            // Keep the concurrency slot until the executor actually exits.
            let _permit = permit;

            // Ensure task ownership and stop signalling are registered first.
            let _ = start_receiver.await;

            let context = super::executor::ExecutionContext::new(
                task_execution_id.clone(),
                ecu_id,
                task_operation_id,
                timeout,
                stop_receiver,
            );

            let mut executor_task = tokio::spawn(async move { executor.execute(context).await });

            let execution_result = if let Some(duration) = timeout {
                match tokio::time::timeout(duration, &mut executor_task).await {
                    Ok(Ok(result)) => result,
                    Ok(Err(join_error)) => Err(super::executor::ExecutionError::Failed(format!(
                        "Executor task terminated unexpectedly: {join_error}"
                    ))),
                    Err(_) => {
                        // Abort only the local executor future. This does not
                        // prove that a remote ECU operation has stopped.
                        executor_task.abort();
                        let _ = executor_task.await;
                        Err(super::executor::ExecutionError::TimedOut)
                    }
                }
            } else {
                match executor_task.await {
                    Ok(result) => result,
                    Err(join_error) => Err(super::executor::ExecutionError::Failed(format!(
                        "Executor task terminated unexpectedly: {join_error}"
                    ))),
                }
            };

            let stop_was_requested = *stop_observer.borrow();

            let (next_state, message) = match execution_result {
                Ok(output) if output.success => (ExecutionState::Completed, output.message),
                Ok(output) => (ExecutionState::Failed, output.message),
                Err(super::executor::ExecutionError::StopRequested) if stop_was_requested => (
                    ExecutionState::Stopped,
                    "Execution stopped by executor".to_string(),
                ),
                Err(super::executor::ExecutionError::Failed(message)) => {
                    (ExecutionState::Failed, message)
                }
                Err(super::executor::ExecutionError::TimedOut) => {
                    (ExecutionState::Failed, "Execution timed out".to_string())
                }
                Err(super::executor::ExecutionError::StopRequested) => (
                    ExecutionState::Failed,
                    "Executor stopped without a stop request".to_string(),
                ),
                Err(super::executor::ExecutionError::UnsupportedCapability(capability)) => (
                    ExecutionState::Failed,
                    format!("Unsupported capability: {capability}"),
                ),
                Err(super::executor::ExecutionError::ControlChannelClosed) => (
                    ExecutionState::Failed,
                    "Execution control channel closed".to_string(),
                ),
            };

            if task_manager
                .transition_execution(&task_execution_id, next_state, current_time_ms())
                .await
                .is_ok()
            {
                task_manager
                    .set_execution_message(&task_execution_id, message)
                    .await;
            }

            task_manager
                .stop_signals
                .lock()
                .await
                .remove(&task_execution_id);

            // Reap this task's own JoinHandle. The start barrier guarantees
            // registration happened before this task can reach cleanup.
            task_manager.tasks.lock().await.remove(&task_execution_id);
        });

        self.stop_signals
            .lock()
            .await
            .insert(execution_id.clone(), stop_sender);

        {
            let mut tasks = self.tasks.lock().await;

            // Reap completed JoinHandles before retaining another one.
            tasks.retain(|_, handle| !handle.is_finished());
            tasks.insert(execution_id, task);
        }

        // Release the task only after its handle and stop signal are stored.
        let _ = start_sender.send(());
        drop(lifecycle_guard);

        Ok(running)
    }

    /// Rejects new work, requests cooperative stops where supported, and waits
    /// up to `grace` for registered tasks to finish.
    ///
    /// Returns execution IDs still running after the grace period. These tasks
    /// are deliberately not force-aborted because a local abort cannot prove
    /// that a remote ECU operation has stopped.
    pub async fn shutdown(&self, grace: Duration) -> Vec<String> {
        {
            let mut lifecycle = self.lifecycle.lock().await;
            *lifecycle = true;
        }

        // Request stop only for operations that explicitly advertise it.
        let stoppable_ids = {
            let state = self.state.lock().await;
            state
                .executions
                .values()
                .filter(|record| {
                    !record.state.is_terminal()
                        && state
                            .operations
                            .get(&record.operation_id)
                            .is_some_and(|operation| operation.capabilities.supports_stop())
                })
                .map(|record| record.execution_id.clone())
                .collect::<Vec<_>>()
        };

        for execution_id in stoppable_ids {
            let sender = self.stop_signals.lock().await.get(&execution_id).cloned();

            let Some(sender) = sender else {
                continue;
            };

            let record = match self.get_execution(&execution_id).await {
                Ok(record) => record,
                Err(_) => continue,
            };

            match record.state {
                ExecutionState::Running | ExecutionState::Frozen => {
                    if self
                        .transition_execution(
                            &execution_id,
                            ExecutionState::StopRequested,
                            current_time_ms(),
                        )
                        .await
                        .is_err()
                    {
                        // The executor may have completed concurrently.
                        continue;
                    }
                }
                ExecutionState::StopRequested => {}
                _ => continue,
            }

            // Publish the signal only after recording the requested transition.
            sender.send_replace(true);
        }

        // Take ownership of handles while waiting. Task cleanup can still
        // acquire the manager's task-map lock and finish normally.
        let mut handles = {
            let mut tasks = self.tasks.lock().await;
            std::mem::take(&mut *tasks)
        };

        let wait_result = tokio::time::timeout(grace, async {
            for handle in handles.values_mut() {
                let _ = handle.await;
            }
        })
        .await;

        let _grace_expired = wait_result.is_err();

        let pending = handles
            .iter()
            .filter(|(_, handle)| !handle.is_finished())
            .map(|(execution_id, _)| execution_id.clone())
            .collect::<Vec<_>>();

        // Retain unresolved handles so a caller can inspect/retry shutdown.
        let mut tasks = self.tasks.lock().await;
        for (execution_id, handle) in handles {
            if !handle.is_finished() {
                tasks.insert(execution_id, handle);
            }
        }

        pending
    }

    /// Requests cooperative termination; does not forcibly abort the task.
    pub async fn request_stop(
        &self,
        execution_id: &str,
    ) -> Result<ExecutionRecord, ExecutionManagerError> {
        let sender = self
            .stop_signals
            .lock()
            .await
            .get(execution_id)
            .cloned()
            .ok_or_else(|| ExecutionManagerError::UnknownExecution(execution_id.to_string()))?;

        let updated = self
            .transition_execution(
                execution_id,
                ExecutionState::StopRequested,
                current_time_ms(),
            )
            .await?;

        sender.send_replace(true);
        Ok(updated)
    }

    async fn set_execution_message(&self, execution_id: &str, message: String) {
        let mut state = self.state.lock().await;

        if let Some(record) = state.executions.get_mut(execution_id) {
            record.message = Some(message);
            record.updated_at_unix_ms = current_time_ms();
        }
    }

    pub async fn get_operation(
        &self,
        operation_id: &str,
    ) -> Result<OperationDefinition, ExecutionManagerError> {
        let state = self.state.lock().await;

        state
            .operations
            .get(operation_id)
            .cloned()
            .ok_or_else(|| ExecutionManagerError::UnknownOperation(operation_id.to_string()))
    }

    pub async fn create_execution(
        &self,
        ecu_id: String,
        operation_id: &str,
        timestamp_unix_ms: u64,
    ) -> Result<ExecutionRecord, ExecutionManagerError> {
        let mut state = self.state.lock().await;

        let operation = state
            .operations
            .get(operation_id)
            .ok_or_else(|| ExecutionManagerError::UnknownOperation(operation_id.to_string()))?;

        if !operation.capabilities.execute {
            return Err(ExecutionManagerError::ExecutionNotSupported(
                operation_id.to_string(),
            ));
        }

        // Evict only terminal records. Never evict active executions.
        if state.executions.len() >= self.capacity {
            let evictable = state.execution_order.iter().position(|id| {
                state
                    .executions
                    .get(id)
                    .is_some_and(|record| record.state.is_terminal())
            });

            let Some(index) = evictable else {
                return Err(ExecutionManagerError::CapacityFull);
            };

            if let Some(expired_id) = state.execution_order.remove(index) {
                state.executions.remove(&expired_id);
            }
        }

        let sequence = self.next_execution_id.fetch_add(1, Ordering::Relaxed);
        let execution_id = format!("exec-{sequence:016}");

        let record = ExecutionRecord {
            execution_id: execution_id.clone(),
            ecu_id,
            operation_id: operation_id.to_string(),
            state: ExecutionState::Created,
            message: None,
            created_at_unix_ms: timestamp_unix_ms,
            updated_at_unix_ms: timestamp_unix_ms,
        };

        state.execution_order.push_back(execution_id.clone());
        state.executions.insert(execution_id, record.clone());

        Ok(record)
    }

    pub async fn transition_execution(
        &self,
        execution_id: &str,
        next: ExecutionState,
        timestamp_unix_ms: u64,
    ) -> Result<ExecutionRecord, ExecutionManagerError> {
        let mut state = self.state.lock().await;

        let record = state
            .executions
            .get(execution_id)
            .ok_or_else(|| ExecutionManagerError::UnknownExecution(execution_id.to_string()))?;

        let operation = state
            .operations
            .get(&record.operation_id)
            .ok_or_else(|| ExecutionManagerError::UnknownOperation(record.operation_id.clone()))?;

        match next {
            ExecutionState::StopRequested if !operation.capabilities.supports_stop() => {
                return Err(ExecutionManagerError::UnsupportedCapability("stop"));
            }
            ExecutionState::Frozen if !operation.capabilities.supports_freeze() => {
                return Err(ExecutionManagerError::UnsupportedCapability("freeze"));
            }
            _ => {}
        }

        let record = state
            .executions
            .get_mut(execution_id)
            .ok_or_else(|| ExecutionManagerError::UnknownExecution(execution_id.to_string()))?;

        record
            .transition_to(next, timestamp_unix_ms)
            .map_err(ExecutionManagerError::InvalidTransition)?;

        Ok(record.clone())
    }

    pub async fn get_execution(
        &self,
        execution_id: &str,
    ) -> Result<ExecutionRecord, ExecutionManagerError> {
        let state = self.state.lock().await;

        state
            .executions
            .get(execution_id)
            .cloned()
            .ok_or_else(|| ExecutionManagerError::UnknownExecution(execution_id.to_string()))
    }

    pub async fn list_executions(&self, ecu_id: &str, operation_id: &str) -> Vec<ExecutionRecord> {
        let state = self.state.lock().await;

        state
            .execution_order
            .iter()
            .filter_map(|id| state.executions.get(id))
            .filter(|record| record.ecu_id == ecu_id && record.operation_id == operation_id)
            .cloned()
            .collect()
    }

    pub async fn active_execution_count(&self) -> usize {
        let state = self.state.lock().await;

        state
            .executions
            .values()
            .filter(|record| !record.state.is_terminal())
            .count()
    }
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::executor::{
        ExecutionContext, ExecutionError, ExecutionFuture, ExecutionOutput, OperationExecutor,
    };
    use crate::execution::model::ExecutionCapabilities;
    use std::sync::Arc;

    struct TestExecutor {
        capabilities: ExecutionCapabilities,
    }

    impl OperationExecutor for TestExecutor {
        fn capabilities(&self) -> ExecutionCapabilities {
            self.capabilities.clone()
        }

        fn execute<'a>(&'a self, _context: ExecutionContext) -> ExecutionFuture<'a> {
            Box::pin(async {
                Ok(ExecutionOutput {
                    success: true,
                    message: "test execution completed".to_string(),
                })
            })
        }
    }

    fn test_executor(capabilities: ExecutionCapabilities) -> Arc<dyn OperationExecutor> {
        Arc::new(TestExecutor { capabilities })
    }

    struct ControlledExecutor {
        capabilities: ExecutionCapabilities,
        delay: std::time::Duration,
        fail: bool,
        wait_for_stop: bool,
    }

    impl OperationExecutor for ControlledExecutor {
        fn capabilities(&self) -> ExecutionCapabilities {
            self.capabilities.clone()
        }

        fn execute<'a>(&'a self, context: ExecutionContext) -> ExecutionFuture<'a> {
            Box::pin(async move {
                if self.wait_for_stop {
                    let mut context = context;
                    context
                        .wait_for_stop()
                        .await
                        .map_err(|_| ExecutionError::ControlChannelClosed)?;

                    return Err(ExecutionError::StopRequested);
                }

                tokio::time::sleep(self.delay).await;

                if self.fail {
                    Err(ExecutionError::Failed(
                        "controlled executor failure".to_string(),
                    ))
                } else {
                    Ok(ExecutionOutput {
                        success: true,
                        message: "controlled execution completed".to_string(),
                    })
                }
            })
        }
    }

    fn controlled_executor(
        delay: std::time::Duration,
        fail: bool,
        wait_for_stop: bool,
    ) -> Arc<dyn OperationExecutor> {
        Arc::new(ControlledExecutor {
            capabilities: all_capabilities(),
            delay,
            fail,
            wait_for_stop,
        })
    }

    async fn wait_for_terminal(manager: &ExecutionManager, execution_id: &str) -> ExecutionRecord {
        for _ in 0..200 {
            let record = manager
                .get_execution(execution_id)
                .await
                .expect("Execution record must remain available");

            if record.state.is_terminal() {
                return record;
            }

            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        panic!("Execution did not reach a terminal state");
    }

    async fn register_controlled_executor(
        manager: &ExecutionManager,
        delay: std::time::Duration,
        fail: bool,
        wait_for_stop: bool,
    ) {
        manager
            .register_operation(operation(all_capabilities()))
            .await
            .expect("Operation registration must succeed");

        manager
            .register_executor(
                "test-operation",
                controlled_executor(delay, fail, wait_for_stop),
            )
            .await
            .expect("Executor registration must succeed");
    }

    fn operation(capabilities: ExecutionCapabilities) -> OperationDefinition {
        OperationDefinition {
            id: "test-operation".to_string(),
            name: "Test operation".to_string(),
            description: "Execution manager test operation".to_string(),
            capabilities,
            asynchronous_execution: true,
        }
    }

    fn all_capabilities() -> ExecutionCapabilities {
        ExecutionCapabilities {
            execute: true,
            stop: true,
            freeze: true,
            reset: true,
            status: true,
        }
    }

    async fn manager(capacity: usize) -> ExecutionManager {
        let manager = ExecutionManager::new(capacity).expect("Manager capacity must be valid");

        manager
            .register_operation(operation(all_capabilities()))
            .await
            .expect("Operation registration must succeed");

        manager
    }

    #[tokio::test]
    async fn creates_and_retrieves_execution() {
        let manager = manager(4).await;

        let created = manager
            .create_execution("ECM".to_string(), "test-operation", 100)
            .await
            .expect("Execution creation must succeed");

        assert_eq!(created.state, ExecutionState::Created);
        assert_eq!(created.ecu_id, "ECM");

        let retrieved = manager
            .get_execution(&created.execution_id)
            .await
            .expect("Execution must be retrievable");

        assert_eq!(retrieved, created);
    }

    #[tokio::test]
    async fn rejects_unknown_operations() {
        let manager = manager(4).await;

        let result = manager
            .create_execution("ECM".to_string(), "unknown", 100)
            .await;

        assert_eq!(
            result,
            Err(ExecutionManagerError::UnknownOperation(
                "unknown".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn rejects_duplicate_operation_registration() {
        let manager = manager(4).await;

        let result = manager
            .register_operation(operation(all_capabilities()))
            .await;

        assert_eq!(
            result,
            Err(ExecutionManagerError::DuplicateOperation(
                "test-operation".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn rejects_unsupported_stop_capability() {
        let manager = ExecutionManager::new(4).expect("Valid capacity");

        manager
            .register_operation(operation(ExecutionCapabilities {
                execute: true,
                stop: false,
                freeze: false,
                reset: false,
                status: true,
            }))
            .await
            .expect("Operation registration");

        let execution = manager
            .create_execution("ECM".to_string(), "test-operation", 100)
            .await
            .expect("Execution creation");

        manager
            .transition_execution(&execution.execution_id, ExecutionState::Running, 110)
            .await
            .expect("Execution should start");

        let result = manager
            .transition_execution(&execution.execution_id, ExecutionState::StopRequested, 120)
            .await;

        assert_eq!(
            result,
            Err(ExecutionManagerError::UnsupportedCapability("stop"))
        );
    }

    #[tokio::test]
    async fn never_evicts_active_execution() {
        let manager = manager(1).await;

        let first = manager
            .create_execution("ECM".to_string(), "test-operation", 100)
            .await
            .expect("First execution");

        manager
            .transition_execution(&first.execution_id, ExecutionState::Running, 110)
            .await
            .expect("First execution starts");

        let second = manager
            .create_execution("BCM".to_string(), "test-operation", 120)
            .await;

        assert_eq!(second, Err(ExecutionManagerError::CapacityFull));

        assert!(manager.get_execution(&first.execution_id).await.is_ok());

        assert_eq!(manager.active_execution_count().await, 1);
    }

    #[tokio::test]
    async fn evicts_oldest_terminal_execution_when_full() {
        let manager = manager(1).await;

        let first = manager
            .create_execution("ECM".to_string(), "test-operation", 100)
            .await
            .expect("First execution");

        manager
            .transition_execution(&first.execution_id, ExecutionState::Running, 110)
            .await
            .expect("First execution starts");

        manager
            .transition_execution(&first.execution_id, ExecutionState::Completed, 120)
            .await
            .expect("First execution completes");

        let second = manager
            .create_execution("BCM".to_string(), "test-operation", 130)
            .await
            .expect("Second execution");

        assert!(manager.get_execution(&first.execution_id).await.is_err());
        assert!(manager.get_execution(&second.execution_id).await.is_ok());
    }

    #[tokio::test]
    async fn filters_execution_history_by_ecu_and_operation() {
        let manager = manager(8).await;

        let first = manager
            .create_execution("ECM".to_string(), "test-operation", 100)
            .await
            .expect("First execution");

        let second = manager
            .create_execution("BCM".to_string(), "test-operation", 110)
            .await
            .expect("Second execution");

        let executions = manager.list_executions("ECM", "test-operation").await;

        assert_eq!(executions.len(), 1);
        assert_eq!(executions[0].execution_id, first.execution_id);
        assert_ne!(executions[0].execution_id, second.execution_id);
    }

    #[tokio::test]
    async fn registers_and_retrieves_executor() {
        let manager = manager(4).await;
        let capabilities = all_capabilities();

        manager
            .register_executor("test-operation", test_executor(capabilities.clone()))
            .await
            .expect("Executor registration must succeed");

        let executor = manager
            .get_executor("test-operation")
            .await
            .expect("Registered executor must be retrievable");

        assert_eq!(executor.capabilities(), capabilities);
    }

    #[tokio::test]
    async fn rejects_executor_capability_mismatch() {
        let manager = manager(4).await;
        let result = manager
            .register_executor(
                "test-operation",
                test_executor(ExecutionCapabilities {
                    execute: true,
                    stop: false,
                    freeze: false,
                    reset: false,
                    status: true,
                }),
            )
            .await;

        assert_eq!(
            result,
            Err(ExecutionManagerError::CapabilityMismatch(
                "test-operation".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn rejects_duplicate_executor_registration() {
        let manager = manager(4).await;
        let capabilities = all_capabilities();

        manager
            .register_executor("test-operation", test_executor(capabilities.clone()))
            .await
            .expect("First registration must succeed");

        let result = manager
            .register_executor("test-operation", test_executor(capabilities))
            .await;

        assert_eq!(
            result,
            Err(ExecutionManagerError::DuplicateExecutor(
                "test-operation".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn rejects_executor_for_unknown_operation() {
        let manager = manager(4).await;
        let result = manager
            .register_executor("unknown-operation", test_executor(all_capabilities()))
            .await;

        assert_eq!(
            result,
            Err(ExecutionManagerError::UnknownOperation(
                "unknown-operation".to_string()
            ))
        );
    }

    #[tokio::test]
    async fn scheduler_marks_successful_execution_completed() {
        let manager = ExecutionManager::new(8).expect("Valid manager");
        register_controlled_executor(&manager, std::time::Duration::from_millis(10), false, false)
            .await;

        let started = manager
            .start_execution("ECM".to_string(), "test-operation", None)
            .await
            .expect("Execution must start");

        assert_eq!(started.state, ExecutionState::Running);

        let completed = wait_for_terminal(&manager, &started.execution_id).await;

        assert_eq!(completed.state, ExecutionState::Completed);
        assert_eq!(
            completed.message.as_deref(),
            Some("controlled execution completed")
        );
    }

    #[tokio::test]
    async fn scheduler_marks_executor_failure_failed() {
        let manager = ExecutionManager::new(8).expect("Valid manager");
        register_controlled_executor(&manager, std::time::Duration::from_millis(10), true, false)
            .await;

        let started = manager
            .start_execution("ECM".to_string(), "test-operation", None)
            .await
            .expect("Execution must start");

        let failed = wait_for_terminal(&manager, &started.execution_id).await;

        assert_eq!(failed.state, ExecutionState::Failed);
        assert_eq!(
            failed.message.as_deref(),
            Some("controlled executor failure")
        );
    }

    #[tokio::test]
    async fn scheduler_marks_timed_out_execution_failed() {
        let manager = ExecutionManager::new(8).expect("Valid manager");
        register_controlled_executor(
            &manager,
            std::time::Duration::from_millis(100),
            false,
            false,
        )
        .await;

        let started = manager
            .start_execution(
                "ECM".to_string(),
                "test-operation",
                Some(std::time::Duration::from_millis(10)),
            )
            .await
            .expect("Execution must start");

        let failed = wait_for_terminal(&manager, &started.execution_id).await;

        assert_eq!(failed.state, ExecutionState::Failed);
        assert_eq!(failed.message.as_deref(), Some("Execution timed out"));
    }

    #[tokio::test]
    async fn scheduler_rejects_work_when_concurrency_limit_is_full() {
        let manager = ExecutionManager::new_with_limits(8, 1).expect("Valid manager limits");

        register_controlled_executor(&manager, std::time::Duration::from_millis(80), false, false)
            .await;

        let first = manager
            .start_execution("ECM".to_string(), "test-operation", None)
            .await
            .expect("First execution must start");

        let second = manager
            .start_execution("BCM".to_string(), "test-operation", None)
            .await;

        assert_eq!(second, Err(ExecutionManagerError::ConcurrencyLimitReached));

        let completed = wait_for_terminal(&manager, &first.execution_id).await;
        assert_eq!(completed.state, ExecutionState::Completed);

        let third = manager
            .start_execution("BCM".to_string(), "test-operation", None)
            .await
            .expect("A slot must become available after completion");

        let third_result = wait_for_terminal(&manager, &third.execution_id).await;
        assert_eq!(third_result.state, ExecutionState::Completed);
    }

    #[tokio::test]
    async fn scheduler_marks_execution_stopped_only_after_executor_acknowledges() {
        let manager = ExecutionManager::new(8).expect("Valid manager");
        register_controlled_executor(&manager, std::time::Duration::ZERO, false, true).await;

        let started = manager
            .start_execution("ECM".to_string(), "test-operation", None)
            .await
            .expect("Execution must start");

        let stop_requested = manager
            .request_stop(&started.execution_id)
            .await
            .expect("Stop request must be accepted");

        assert_eq!(stop_requested.state, ExecutionState::StopRequested);

        let stopped = wait_for_terminal(&manager, &started.execution_id).await;
        assert_eq!(stopped.state, ExecutionState::Stopped);
        assert_eq!(
            stopped.message.as_deref(),
            Some("Execution stopped by executor")
        );
    }

    #[tokio::test]
    async fn scheduler_rejects_zero_timeout() {
        let manager = ExecutionManager::new(8).expect("Valid manager");
        register_controlled_executor(&manager, std::time::Duration::from_millis(10), false, false)
            .await;

        let result = manager
            .start_execution(
                "ECM".to_string(),
                "test-operation",
                Some(std::time::Duration::ZERO),
            )
            .await;

        assert_eq!(result, Err(ExecutionManagerError::InvalidTimeout));
    }

    #[test]
    fn rejects_zero_capacity() {
        assert_eq!(
            ExecutionManager::new(0).err(),
            Some(ExecutionManagerError::InvalidCapacity)
        );
    }

    struct PanickingExecutor {
        capabilities: ExecutionCapabilities,
    }

    impl OperationExecutor for PanickingExecutor {
        fn capabilities(&self) -> ExecutionCapabilities {
            self.capabilities.clone()
        }

        fn execute<'a>(&'a self, _context: ExecutionContext) -> ExecutionFuture<'a> {
            Box::pin(async {
                panic!("intentional executor panic for supervision test");
            })
        }
    }

    #[tokio::test]
    async fn scheduler_contains_executor_panic_and_cleans_up_task() {
        let manager =
            ExecutionManager::new_with_limits(4, 1).expect("Manager limits must be valid");

        manager
            .register_operation(operation(all_capabilities()))
            .await
            .expect("Operation registration must succeed");

        manager
            .register_executor(
                "test-operation",
                Arc::new(PanickingExecutor {
                    capabilities: all_capabilities(),
                }),
            )
            .await
            .expect("Executor registration must succeed");

        let started = manager
            .start_execution("ECM".to_string(), "test-operation", None)
            .await
            .expect("Execution must start");

        let record = wait_for_terminal(&manager, &started.execution_id).await;

        assert_eq!(record.state, ExecutionState::Failed);
        assert!(
            record
                .message
                .as_deref()
                .is_some_and(|message| !message.is_empty()),
            "Panic failure must be recorded"
        );

        for _ in 0..100 {
            if manager.tasks.lock().await.is_empty() && manager.stop_signals.lock().await.is_empty()
            {
                break;
            }

            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        assert!(manager.tasks.lock().await.is_empty());
        assert!(manager.stop_signals.lock().await.is_empty());
    }

    #[tokio::test]
    async fn shutdown_rejects_new_execution_requests() {
        let manager = manager(4).await;

        let pending = manager
            .shutdown(std::time::Duration::from_millis(100))
            .await;

        assert!(pending.is_empty());

        let result = manager
            .start_execution("ECM".to_string(), "test-operation", None)
            .await;

        assert!(matches!(
            result,
            Err(ExecutionManagerError::ManagerShuttingDown)
        ));
    }

    #[tokio::test]
    async fn shutdown_waits_for_cooperative_executor_stop_acknowledgement() {
        let manager = ExecutionManager::new(4).expect("Manager capacity must be valid");
        register_controlled_executor(&manager, std::time::Duration::ZERO, false, true).await;

        let started = manager
            .start_execution("ECM".to_string(), "test-operation", None)
            .await
            .expect("Execution must start");

        let pending = manager.shutdown(std::time::Duration::from_secs(1)).await;

        assert!(pending.is_empty());

        let record = manager
            .get_execution(&started.execution_id)
            .await
            .expect("Execution record must remain available");

        assert_eq!(record.state, ExecutionState::Stopped);
        assert!(manager.tasks.lock().await.is_empty());
    }

    #[tokio::test]
    async fn shutdown_reports_non_cooperative_execution_after_grace_period() {
        let manager = ExecutionManager::new(4).expect("Manager capacity must be valid");
        register_controlled_executor(
            &manager,
            std::time::Duration::from_millis(200),
            false,
            false,
        )
        .await;

        let started = manager
            .start_execution("ECM".to_string(), "test-operation", None)
            .await
            .expect("Execution must start");

        let pending = manager.shutdown(std::time::Duration::from_millis(10)).await;

        assert_eq!(pending, vec![started.execution_id.clone()]);

        let record = wait_for_terminal(&manager, &started.execution_id).await;

        // The executor ignored the stop signal. Do not report it as stopped.
        assert_eq!(record.state, ExecutionState::Completed);
    }
}
