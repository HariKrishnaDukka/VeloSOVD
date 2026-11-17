use super::model::ExecutionCapabilities;
use std::{future::Future, pin::Pin, time::Duration};
use tokio::sync::watch;

/// Boxed future used by dynamically registered operation executors.
pub type ExecutionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ExecutionOutput, ExecutionError>> + Send + 'a>>;

/// Runtime context supplied to every operation execution.
pub struct ExecutionContext {
    pub execution_id: String,
    pub ecu_id: String,
    pub operation_id: String,
    pub timeout: Option<Duration>,
    stop_requested: watch::Receiver<bool>,
}

impl ExecutionContext {
    pub fn new(
        execution_id: String,
        ecu_id: String,
        operation_id: String,
        timeout: Option<Duration>,
        stop_requested: watch::Receiver<bool>,
    ) -> Self {
        Self {
            execution_id,
            ecu_id,
            operation_id,
            timeout,
            stop_requested,
        }
    }

    /// Checks whether cooperative termination has been requested.
    pub fn is_stop_requested(&self) -> bool {
        *self.stop_requested.borrow()
    }

    /// Waits until cooperative termination is requested.
    pub async fn wait_for_stop(&mut self) -> Result<(), ExecutionError> {
        loop {
            if *self.stop_requested.borrow() {
                return Ok(());
            }

            if self.stop_requested.changed().await.is_err() {
                return Err(ExecutionError::ControlChannelClosed);
            }
        }
    }
}

/// Result returned by a completed operation executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionOutput {
    pub success: bool,
    pub message: String,
}

/// Errors reported by an operation executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionError {
    Failed(String),
    StopRequested,
    TimedOut,
    UnsupportedCapability(&'static str),
    ControlChannelClosed,
}

/// Generic interface for asynchronously executable vehicle operations.
///
/// Implementations must check the execution context at safe interruption
/// points. Dropping an HTTP request or Tokio task does not prove that a
/// remote ECU operation was cancelled.
pub trait OperationExecutor: Send + Sync {
    fn capabilities(&self) -> ExecutionCapabilities;

    fn execute<'a>(&'a self, context: ExecutionContext) -> ExecutionFuture<'a>;
}
