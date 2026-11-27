//! M3 step 2: operation rules. No network, no GUI - plain functions the Tauri shell calls
//! before it sends anything, so they are unit-tested in CI.
//!
//! What central really offers today (read from its code, not assumed):
//! - `GET /api/ecus/{id}/operations` lists operations (id, name, description,
//!   `supports_execution`). It does NOT publish capabilities.
//! - Start: `POST /sovd/v1/components/{id}/operations/{op}/executions` with a JSON body.
//! - Status: `GET .../executions/{exec}` answers `running`, `completed` or `failed`.
//! - Stop: `DELETE .../executions/{exec}`; the only operation (`clear-faults`) cannot stop.
//! - History: the `/api` list `GET /api/ecus/{id}/operations/{op}/executions` (read only).
//!
//! So Studio offers Stop only when an operation lists `capabilities.stop = true`, which no
//! operation does yet.

use serde::{Deserialize, Serialize};

use crate::faults::{check_segment, may_change};
use crate::session::{StudioError, validate_path};

/// What an operation declares it can do. Absent in today's central answers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct OperationCapabilities {
    pub execute: bool,
    pub stop: bool,
    pub freeze: bool,
    pub reset: bool,
    pub status: bool,
}

/// One entry of the operation list. Unknown fields from central are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct OperationInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub supports_execution: bool,
    #[serde(default)]
    pub capabilities: Option<OperationCapabilities>,
}

/// Parse the body of `GET /api/ecus/{id}/operations`.
pub fn parse_operations(body: &str) -> Result<Vec<OperationInfo>, StudioError> {
    serde_json::from_str(body)
        .map_err(|_| StudioError::Transport("central sent an unreadable operation list".into()))
}

/// May this role start operations? Technician and up (central enforces the same rule).
pub fn may_start(role: &str) -> bool {
    may_change(role)
}

/// May this role stop an execution? Technician and up, and only if the operation declares it.
/// Unknown capabilities mean no.
pub fn may_stop(role: &str, capabilities: Option<&OperationCapabilities>) -> bool {
    may_change(role) && capabilities.is_some_and(|c| c.stop)
}

fn checked(path: String) -> Result<String, StudioError> {
    validate_path(&path)?;
    Ok(path)
}

/// `GET` the operations of one ECU.
pub fn descriptors_path(ecu: &str) -> Result<String, StudioError> {
    check_segment("ECU id", ecu)?;
    checked(format!("/api/ecus/{ecu}/operations"))
}

/// `GET` the execution history of one operation (the `/api` list, read only).
pub fn history_path(ecu: &str, operation: &str) -> Result<String, StudioError> {
    check_segment("ECU id", ecu)?;
    check_segment("operation id", operation)?;
    checked(format!("/api/ecus/{ecu}/operations/{operation}/executions"))
}

/// `POST` starts an execution.
pub fn start_path(ecu: &str, operation: &str) -> Result<String, StudioError> {
    check_segment("ECU id", ecu)?;
    check_segment("operation id", operation)?;
    checked(format!(
        "/sovd/v1/components/{ecu}/operations/{operation}/executions"
    ))
}

/// `GET` the status of one execution, `DELETE` stops it.
pub fn execution_path(ecu: &str, operation: &str, execution: &str) -> Result<String, StudioError> {
    check_segment("ECU id", ecu)?;
    check_segment("operation id", operation)?;
    check_segment("execution id", execution)?;
    checked(format!(
        "/sovd/v1/components/{ecu}/operations/{operation}/executions/{execution}"
    ))
}

/// A request that passed every check and may now be sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequestPlan {
    pub method: &'static str,
    pub path: String,
    pub body: Option<String>,
}

/// Plan a start. Role first, then path. Central needs a JSON body; `{}` is a plain start.
pub fn plan_start(role: &str, ecu: &str, operation: &str) -> Result<RequestPlan, StudioError> {
    if !may_start(role) {
        return Err(StudioError::InvalidRequest(
            "your role may not start operations".into(),
        ));
    }
    Ok(RequestPlan {
        method: "POST",
        path: start_path(ecu, operation)?,
        body: Some("{}".into()),
    })
}

/// Plan a stop. Role first, then capability, then path.
pub fn plan_stop(
    role: &str,
    capabilities: Option<&OperationCapabilities>,
    ecu: &str,
    operation: &str,
    execution: &str,
) -> Result<RequestPlan, StudioError> {
    if !may_change(role) {
        return Err(StudioError::InvalidRequest(
            "your role may not stop operations".into(),
        ));
    }
    if !capabilities.is_some_and(|c| c.stop) {
        return Err(StudioError::InvalidRequest(
            "this operation cannot be stopped".into(),
        ));
    }
    Ok(RequestPlan {
        method: "DELETE",
        path: execution_path(ecu, operation, execution)?,
        body: None,
    })
}

/// The three states central reports on `/sovd/v1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    Running,
    Completed,
    Failed,
}

impl RunState {
    /// Polling stops when this is true.
    pub fn is_finished(self) -> bool {
        self != Self::Running
    }
}

#[derive(Deserialize)]
struct StatusBody {
    status: String,
}

#[derive(Deserialize)]
struct StartBody {
    id: String,
}

/// Parse the body of `GET .../executions/{exec}`.
pub fn parse_run_state(body: &str) -> Result<RunState, StudioError> {
    let unreadable =
        || StudioError::Transport("central sent an unreadable execution status".into());
    let parsed: StatusBody = serde_json::from_str(body).map_err(|_| unreadable())?;
    match parsed.status.as_str() {
        "running" => Ok(RunState::Running),
        "completed" => Ok(RunState::Completed),
        "failed" => Ok(RunState::Failed),
        _ => Err(unreadable()),
    }
}

/// Parse the execution id out of the answer to a start request.
pub fn parse_start_id(body: &str) -> Result<String, StudioError> {
    let parsed: StartBody = serde_json::from_str(body)
        .map_err(|_| StudioError::Transport("central sent an unreadable start answer".into()))?;
    check_segment("execution id", &parsed.id)?;
    Ok(parsed.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_sovd_v1_execution_paths() {
        assert_eq!(
            start_path("ECM", "clear-faults").unwrap(),
            "/sovd/v1/components/ECM/operations/clear-faults/executions"
        );
        assert_eq!(
            execution_path("ECM", "clear-faults", "exec-1").unwrap(),
            "/sovd/v1/components/ECM/operations/clear-faults/executions/exec-1"
        );
        assert_eq!(
            history_path("ECM", "clear-faults").unwrap(),
            "/api/ecus/ECM/operations/clear-faults/executions"
        );
        assert_eq!(
            descriptors_path("HVAC").unwrap(),
            "/api/ecus/HVAC/operations"
        );

        let long = "A".repeat(65);
        for bad in [
            "",
            "..",
            "a/b",
            "a?b",
            "a#b",
            "a b",
            "a\\b",
            "x/../y",
            "\u{e9}",
            "a\nb",
            long.as_str(),
        ] {
            assert!(descriptors_path(bad).is_err(), "{bad:?}");
            assert!(history_path(bad, "op").is_err(), "{bad:?}");
            assert!(history_path("ECM", bad).is_err(), "{bad:?}");
            assert!(start_path(bad, "op").is_err(), "{bad:?}");
            assert!(start_path("ECM", bad).is_err(), "{bad:?}");
            assert!(execution_path(bad, "op", "e1").is_err(), "{bad:?}");
            assert!(execution_path("ECM", bad, "e1").is_err(), "{bad:?}");
            assert!(execution_path("ECM", "op", bad).is_err(), "{bad:?}");
        }
        for ok in [
            start_path("LIGHTING", "clear-faults").unwrap(),
            execution_path("ECM", "clear-faults", "exec-1").unwrap(),
            history_path("ECM", "clear-faults").unwrap(),
        ] {
            assert!(validate_path(&ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn stop_only_when_capability_listed() {
        let can_stop = OperationCapabilities {
            stop: true,
            ..Default::default()
        };
        // what central's clear-faults executor declares internally
        let no_stop = OperationCapabilities {
            execute: true,
            status: true,
            ..Default::default()
        };
        for role in ["technician", "engineer"] {
            assert!(may_stop(role, Some(&can_stop)), "{role}");
            assert!(!may_stop(role, Some(&no_stop)), "{role}");
            assert!(!may_stop(role, None), "{role}");
        }
        for role in ["viewer", "", "admin", "Technician"] {
            assert!(!may_stop(role, Some(&can_stop)), "{role:?}");
        }

        assert!(plan_stop("viewer", Some(&can_stop), "ECM", "op", "exec-1").is_err());
        assert!(plan_stop("technician", Some(&no_stop), "ECM", "op", "exec-1").is_err());
        assert!(plan_stop("technician", None, "ECM", "op", "exec-1").is_err());
        let plan = plan_stop("technician", Some(&can_stop), "ECM", "op", "exec-1").unwrap();
        assert_eq!(
            plan,
            RequestPlan {
                method: "DELETE",
                path: "/sovd/v1/components/ECM/operations/op/executions/exec-1".into(),
                body: None,
            }
        );
    }

    #[test]
    fn start_needs_technician_and_plans_post() {
        for role in ["viewer", "", "admin"] {
            assert!(plan_start(role, "ECM", "clear-faults").is_err(), "{role:?}");
            assert!(!may_start(role), "{role:?}");
        }
        for role in ["technician", "engineer"] {
            let plan = plan_start(role, "ECM", "clear-faults").unwrap();
            assert_eq!(plan.method, "POST");
            assert_eq!(
                plan.path,
                "/sovd/v1/components/ECM/operations/clear-faults/executions"
            );
            assert_eq!(plan.body.as_deref(), Some("{}"));
        }
        assert!(plan_start("technician", "E/M", "clear-faults").is_err());
        assert!(plan_start("technician", "ECM", "").is_err());
    }

    #[test]
    fn parses_sovd_status_and_start_response() {
        let status = |s: &str| format!(r#"{{"status":"{s}","capability":"execute"}}"#);
        assert_eq!(
            parse_run_state(&status("running")).unwrap(),
            RunState::Running
        );
        assert_eq!(
            parse_run_state(&status("completed")).unwrap(),
            RunState::Completed
        );
        assert_eq!(
            parse_run_state(&status("failed")).unwrap(),
            RunState::Failed
        );
        assert!(parse_run_state(&status("paused")).is_err());
        assert!(parse_run_state("not json").is_err());
        assert!(parse_run_state("{}").is_err());

        assert!(!RunState::Running.is_finished());
        assert!(RunState::Completed.is_finished());
        assert!(RunState::Failed.is_finished());

        assert_eq!(
            parse_start_id(r#"{"id":"exec-7","status":"running"}"#).unwrap(),
            "exec-7"
        );
        assert!(parse_start_id(r#"{"id":"../x","status":"running"}"#).is_err());
        assert!(parse_start_id("{}").is_err());
        assert!(parse_start_id("").is_err());
    }

    #[test]
    fn parses_operation_list_without_capabilities() {
        let body = r#"[{"id":"clear-faults","name":"Clear fault memory","method":"POST","execution_path":"/api/ecus/ECM/operations/clear-faults/executions","description":"Clear all stored diagnostic faults for this ECU","supports_execution":true}]"#;
        let ops = parse_operations(body).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].id, "clear-faults");
        assert!(ops[0].supports_execution);
        assert!(ops[0].capabilities.is_none());
        // central publishes no capabilities today, so Stop is never offered
        assert!(!may_stop("engineer", ops[0].capabilities.as_ref()));

        let with = r#"[{"id":"x","name":"X","description":"d","supports_execution":true,"capabilities":{"stop":true}}]"#;
        let ops = parse_operations(with).unwrap();
        assert!(may_stop("technician", ops[0].capabilities.as_ref()));
        assert!(!may_stop("viewer", ops[0].capabilities.as_ref()));

        assert!(parse_operations("{}").is_err());
        assert!(parse_operations("").is_err());
    }
}
