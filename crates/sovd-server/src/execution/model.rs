use serde::Serialize;
use std::collections::BTreeSet;

/// Internal lifecycle states for a generic operation execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionState {
    Created,
    Running,
    StopRequested,
    Frozen,
    Completed,
    Failed,
    Stopped,
}

impl ExecutionState {
    /// Returns true when no further lifecycle transition is permitted.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Stopped)
    }

    /// Validates an internal lifecycle transition.
    pub fn can_transition_to(self, next: Self) -> bool {
        use ExecutionState::*;

        matches!(
            (self, next),
            (Created, Running)
                | (Running, StopRequested)
                | (Running, Frozen)
                | (Running, Completed)
                | (Running, Failed)
                | (StopRequested, Stopped)
                | (StopRequested, Completed)
                | (StopRequested, Failed)
                | (Frozen, Running)
                | (Frozen, StopRequested)
                | (Frozen, Failed)
        )
    }
}

/// Capabilities actually implemented by a registered operation executor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionCapabilities {
    pub execute: bool,
    pub stop: bool,
    pub freeze: bool,
    pub reset: bool,
    pub status: bool,
}

impl ExecutionCapabilities {
    pub fn supports_stop(&self) -> bool {
        self.stop
    }

    pub fn supports_freeze(&self) -> bool {
        self.freeze
    }

    pub fn supports_reset(&self) -> bool {
        self.reset
    }
}

/// Generic operation registration metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperationDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub capabilities: ExecutionCapabilities,
    pub asynchronous_execution: bool,
}

/// Generic execution record owned by the execution manager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionRecord {
    pub execution_id: String,
    pub ecu_id: String,
    pub operation_id: String,
    pub state: ExecutionState,
    pub message: Option<String>,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

impl ExecutionRecord {
    pub fn transition_to(
        &mut self,
        next: ExecutionState,
        timestamp_unix_ms: u64,
    ) -> Result<(), ExecutionTransitionError> {
        if !self.state.can_transition_to(next) {
            return Err(ExecutionTransitionError {
                from: self.state,
                to: next,
            });
        }

        self.state = next;
        self.updated_at_unix_ms = timestamp_unix_ms;
        Ok(())
    }
}

/// Explicit error for invalid lifecycle transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionTransitionError {
    pub from: ExecutionState,
    pub to: ExecutionState,
}

/// Capability identifiers supported by the SOVD execution model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExecutionCapability {
    Execute,
    Stop,
    Freeze,
    Reset,
    Status,
}

impl ExecutionCapabilities {
    pub fn supported(&self) -> BTreeSet<ExecutionCapability> {
        let mut result = BTreeSet::new();

        if self.execute {
            result.insert(ExecutionCapability::Execute);
        }
        if self.stop {
            result.insert(ExecutionCapability::Stop);
        }
        if self.freeze {
            result.insert(ExecutionCapability::Freeze);
        }
        if self.reset {
            result.insert(ExecutionCapability::Reset);
        }
        if self.status {
            result.insert(ExecutionCapability::Status);
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> ExecutionRecord {
        ExecutionRecord {
            execution_id: "exec-test-001".to_string(),
            ecu_id: "ECM".to_string(),
            operation_id: "test-operation".to_string(),
            state: ExecutionState::Created,
            message: None,
            created_at_unix_ms: 100,
            updated_at_unix_ms: 100,
        }
    }

    #[test]
    fn permits_normal_execution_lifecycle() {
        let mut execution = record();

        assert!(
            execution
                .transition_to(ExecutionState::Running, 110)
                .is_ok()
        );
        assert!(
            execution
                .transition_to(ExecutionState::Completed, 120)
                .is_ok()
        );

        assert_eq!(execution.state, ExecutionState::Completed);
        assert_eq!(execution.updated_at_unix_ms, 120);
        assert!(execution.state.is_terminal());
    }

    #[test]
    fn permits_confirmed_stop_lifecycle() {
        let mut execution = record();

        assert!(
            execution
                .transition_to(ExecutionState::Running, 110)
                .is_ok()
        );
        assert!(
            execution
                .transition_to(ExecutionState::StopRequested, 120)
                .is_ok()
        );
        assert!(
            execution
                .transition_to(ExecutionState::Stopped, 130)
                .is_ok()
        );

        assert_eq!(execution.state, ExecutionState::Stopped);
        assert!(execution.state.is_terminal());
    }

    #[test]
    fn rejects_completion_without_running() {
        let mut execution = record();

        let result = execution.transition_to(ExecutionState::Completed, 120);

        assert_eq!(
            result,
            Err(ExecutionTransitionError {
                from: ExecutionState::Created,
                to: ExecutionState::Completed,
            })
        );
        assert_eq!(execution.state, ExecutionState::Created);
    }

    #[test]
    fn rejects_transitions_out_of_terminal_states() {
        let mut execution = record();

        assert!(
            execution
                .transition_to(ExecutionState::Running, 110)
                .is_ok()
        );
        assert!(
            execution
                .transition_to(ExecutionState::Completed, 120)
                .is_ok()
        );

        assert!(
            execution
                .transition_to(ExecutionState::Running, 130)
                .is_err()
        );
    }

    #[test]
    fn permits_freeze_and_resume_when_executor_supports_them() {
        let mut execution = record();

        assert!(
            execution
                .transition_to(ExecutionState::Running, 110)
                .is_ok()
        );
        assert!(execution.transition_to(ExecutionState::Frozen, 120).is_ok());
        assert!(
            execution
                .transition_to(ExecutionState::Running, 130)
                .is_ok()
        );

        assert_eq!(execution.state, ExecutionState::Running);
    }

    #[test]
    fn exposes_only_declared_capabilities() {
        let capabilities = ExecutionCapabilities {
            execute: true,
            stop: true,
            freeze: false,
            reset: false,
            status: true,
        };

        let supported = capabilities.supported();

        assert!(supported.contains(&ExecutionCapability::Execute));
        assert!(supported.contains(&ExecutionCapability::Stop));
        assert!(!supported.contains(&ExecutionCapability::Freeze));
        assert!(!supported.contains(&ExecutionCapability::Reset));
        assert!(supported.contains(&ExecutionCapability::Status));
    }
}
