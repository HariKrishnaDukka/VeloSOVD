//! Operation execution handlers (`/api` and `/sovd/v1`) and the clear-faults executor.

use super::*;
use crate::execution::{
    executor::{
        ExecutionContext, ExecutionError, ExecutionFuture, ExecutionOutput, OperationExecutor,
    },
    manager::{ExecutionManager, ExecutionManagerError},
    model::{ExecutionCapabilities, ExecutionState, OperationDefinition},
};
use axum::{
    Json,
    extract::{Path, State},
    http::{StatusCode, header},
};
use reqwest::Client;
use std::{sync::Arc, time::Duration};
use vehicle_topology::VehicleTopology;

/// Upper bound for the legacy synchronous `/api` execution call. Keeps the
/// request below the R3 server deadline (10 s).
pub(crate) const LEGACY_SYNC_WAIT: Duration = Duration::from_secs(8);

/// Legacy `/api` view of a record held by the single ExecutionManager store.
pub(crate) fn legacy_execution_view(
    record: crate::execution::model::ExecutionRecord,
) -> OperationExecution {
    OperationExecution {
        execution_id: record.execution_id,
        ecu_id: record.ecu_id,
        operation: record.operation_id,
        status: execution_manager_status(record.state).to_string(),
        success: record.state == ExecutionState::Completed,
        // Failed executions store a GenericError as JSON; show its human text.
        message: record
            .message
            .as_deref()
            .map(|m| generic_error_from_execution_message(m).message)
            .unwrap_or_default(),
        created_at_unix_ms: record.created_at_unix_ms,
    }
}

/// Wait until an execution reaches a terminal state (or the legacy deadline).
pub(crate) async fn wait_for_terminal(
    manager: &ExecutionManager,
    execution_id: &str,
) -> Result<crate::execution::model::ExecutionRecord, StatusCode> {
    let deadline = tokio::time::Instant::now() + LEGACY_SYNC_WAIT;
    loop {
        let record = manager
            .get_execution(execution_id)
            .await
            .map_err(manager_error_status)?;
        if record.state.is_terminal() || tokio::time::Instant::now() >= deadline {
            return Ok(record);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub(crate) async fn execute_clear_faults(
    State(state): State<AppState>,
    Path((id, operation_id)): Path<(String, String)>,
) -> Result<Json<OperationExecution>, StatusCode> {
    // R5: the legacy route is a synchronous wrapper over the same
    // ExecutionManager used by /sovd/v1 - one store, one history.
    let _ = resolve_ecu_url(&state, &id)?;
    let manager = get_execution_manager(&state).await?;
    let started = manager
        .start_execution(id, &operation_id, None)
        .await
        .map_err(manager_error_status)?;
    let record = wait_for_terminal(manager, &started.execution_id).await?;
    Ok(Json(legacy_execution_view(record)))
}

/// What the clear-faults operation can do. One definition: the executor, the operation
/// registration and the published operation list all use it, so they cannot disagree.
/// Stop is not offered: cancelling the HTTP future cannot prove the remote ECU stopped.
pub(crate) fn clear_faults_capabilities() -> ExecutionCapabilities {
    ExecutionCapabilities {
        execute: true,
        stop: false,
        freeze: false,
        reset: false,
        status: true,
    }
}

/// Executor for the demonstrator's existing zonal clear-faults operation.
/// Stop/freeze/reset are intentionally not advertised: cancelling this HTTP
/// future cannot prove that the remote ECU stopped processing the request.
pub(crate) struct ClearFaultsExecutor {
    pub(crate) client: Client,
    pub(crate) topology: VehicleTopology,
    pub(crate) zonal_http_port: u16,
    pub(crate) zonal_scheme: &'static str,
}

impl OperationExecutor for ClearFaultsExecutor {
    fn capabilities(&self) -> ExecutionCapabilities {
        clear_faults_capabilities()
    }

    fn execute<'a>(&'a self, context: ExecutionContext) -> ExecutionFuture<'a> {
        Box::pin(async move {
            let ecu = self
                .topology
                .ecus
                .iter()
                .find(|ecu| ecu.id == context.ecu_id)
                .ok_or_else(|| ExecutionError::Failed("Unknown ECU".to_string()))?;

            let zone = self
                .topology
                .zones
                .iter()
                .find(|zone| zone.id == ecu.zone)
                .ok_or_else(|| ExecutionError::Failed("ECU zone is not configured".to_string()))?;

            let url = format!(
                "{}://{}:{}/ecus/{}/diagnostics/operations/clear-faults",
                self.zonal_scheme, zone.zonal_ip, self.zonal_http_port, ecu.id
            );

            invoke_clear_faults(&self.client, url)
                .await
                .map_err(|error| ExecutionError::Failed(error.execution_message()))?;

            Ok(ExecutionOutput {
                success: true,
                message: format!("{} fault memory cleared", context.ecu_id),
            })
        })
    }
}

pub(crate) async fn get_execution_manager(
    state: &AppState,
) -> Result<&ExecutionManager, StatusCode> {
    state
        .execution_manager
        .get_or_try_init(|| async {
            let manager = ExecutionManager::new_with_limits(EXECUTION_HISTORY_CAPACITY, 8)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            let capabilities = clear_faults_capabilities();

            manager
                .register_operation(OperationDefinition {
                    id: "clear-faults".to_string(),
                    name: "Clear diagnostic faults".to_string(),
                    description: "Clear the ECU diagnostic fault memory".to_string(),
                    capabilities: capabilities.clone(),
                    asynchronous_execution: true,
                })
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            manager
                .register_executor(
                    "clear-faults",
                    Arc::new(ClearFaultsExecutor {
                        client: state.client.clone(),
                        topology: state.topology.clone(),
                        zonal_http_port: state.zonal_http_port,
                        zonal_scheme: state.zonal_scheme,
                    }),
                )
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            Ok::<ExecutionManager, StatusCode>(manager)
        })
        .await
}

pub(crate) fn execution_manager_status(state: ExecutionState) -> &'static str {
    match state {
        ExecutionState::Created
        | ExecutionState::Running
        | ExecutionState::StopRequested
        | ExecutionState::Frozen => "running",
        ExecutionState::Completed | ExecutionState::Stopped => "completed",
        ExecutionState::Failed => "failed",
    }
}

pub(crate) fn manager_error_status(error: ExecutionManagerError) -> StatusCode {
    match error {
        ExecutionManagerError::UnknownOperation(_)
        | ExecutionManagerError::UnknownExecution(_)
        | ExecutionManagerError::ExecutorNotRegistered(_) => StatusCode::NOT_FOUND,
        ExecutionManagerError::UnsupportedCapability(_) => StatusCode::NOT_IMPLEMENTED,
        ExecutionManagerError::ConcurrencyLimitReached | ExecutionManagerError::CapacityFull => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        ExecutionManagerError::InvalidTimeout => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub(crate) async fn sovd_start_operation_execution(
    State(state): State<AppState>,
    Path((entity_collection, id, operation_id)): Path<(String, String, String)>,
    Json(request): Json<SovdStartExecutionRequest>,
) -> Result<
    (
        StatusCode,
        [(header::HeaderName, String); 1],
        Json<SovdStartExecutionResponse>,
    ),
    StatusCode,
> {
    if entity_collection != "components" {
        return Err(StatusCode::NOT_FOUND);
    }

    let _ = resolve_ecu_url(&state, &id)?;

    if request.proximity_response.is_some() {
        return Err(StatusCode::NOT_IMPLEMENTED);
    }

    if request.parameters.is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let timeout = request.timeout.map(Duration::from_secs);
    let manager = get_execution_manager(&state).await?;

    let execution = manager
        .start_execution(id, &operation_id, timeout)
        .await
        .map_err(manager_error_status)?;

    let location = format!(
        "/sovd/v1/components/{}/operations/{}/executions/{}",
        execution.ecu_id, execution.operation_id, execution.execution_id
    );

    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, location)],
        Json(SovdStartExecutionResponse {
            id: execution.execution_id,
            status: "running".to_string(),
        }),
    ))
}

pub(crate) async fn list_operation_executions(
    State(state): State<AppState>,
    Path((id, operation_id)): Path<(String, String)>,
) -> Result<Json<Vec<OperationExecution>>, StatusCode> {
    let _ = resolve_ecu_url(&state, &id)?;
    let manager = get_execution_manager(&state).await?;
    let executions = manager
        .list_executions(&id, &operation_id)
        .await
        .into_iter()
        .map(legacy_execution_view)
        .collect();
    Ok(Json(executions))
}

pub(crate) async fn get_operation_execution(
    State(state): State<AppState>,
    Path((id, operation_id, execution_id)): Path<(String, String, String)>,
) -> Result<Json<OperationExecution>, StatusCode> {
    let _ = resolve_ecu_url(&state, &id)?;
    let manager = get_execution_manager(&state).await?;
    let record = manager
        .get_execution(&execution_id)
        .await
        .map_err(manager_error_status)?;
    if record.ecu_id != id || record.operation_id != operation_id {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(Json(legacy_execution_view(record)))
}

pub(crate) async fn sovd_list_operation_executions(
    State(state): State<AppState>,
    Path((entity_collection, id, operation_id)): Path<(String, String, String)>,
) -> Result<Json<SovdExecutionIds>, StatusCode> {
    if entity_collection != "components" {
        return Err(StatusCode::NOT_FOUND);
    }

    let _ = resolve_ecu_url(&state, &id)?;
    let manager = get_execution_manager(&state).await?;
    let records = manager.list_executions(&id, &operation_id).await;

    Ok(Json(SovdExecutionIds {
        items: records
            .into_iter()
            .map(|record| record.execution_id)
            .collect(),
    }))
}

pub(crate) async fn sovd_get_operation_execution(
    State(state): State<AppState>,
    Path((entity_collection, id, operation_id, execution_id)): Path<(
        String,
        String,
        String,
        String,
    )>,
) -> Result<Json<SovdExecutionStatus>, StatusCode> {
    if entity_collection != "components" {
        return Err(StatusCode::NOT_FOUND);
    }

    let _ = resolve_ecu_url(&state, &id)?;
    let manager = get_execution_manager(&state).await?;
    let execution = manager
        .get_execution(&execution_id)
        .await
        .map_err(manager_error_status)?;

    if execution.ecu_id != id || execution.operation_id != operation_id {
        return Err(StatusCode::NOT_FOUND);
    }

    let error = if execution.state == ExecutionState::Failed {
        Some(vec![SovdExecutionError {
            path: "/".to_string(),
            error: generic_error_from_execution_message(
                execution
                    .message
                    .as_deref()
                    .unwrap_or("Operation execution failed"),
            ),
        }])
    } else {
        None
    };

    Ok(Json(SovdExecutionStatus {
        status: execution_manager_status(execution.state).to_string(),
        capability: "execute".to_string(),
        error,
    }))
}

pub(crate) async fn sovd_delete_operation_execution(
    State(state): State<AppState>,
    Path((entity_collection, id, operation_id, execution_id)): Path<(
        String,
        String,
        String,
        String,
    )>,
) -> Result<StatusCode, StatusCode> {
    if entity_collection != "components" {
        return Err(StatusCode::NOT_FOUND);
    }

    let _ = resolve_ecu_url(&state, &id)?;
    let manager = get_execution_manager(&state).await?;
    let execution = manager
        .get_execution(&execution_id)
        .await
        .map_err(manager_error_status)?;

    if execution.ecu_id != id || execution.operation_id != operation_id {
        return Err(StatusCode::NOT_FOUND);
    }

    manager
        .request_stop(&execution_id)
        .await
        .map_err(manager_error_status)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn sovd_put_execution_capability(
    State(state): State<AppState>,
    Path((entity_collection, id, operation_id, execution_id)): Path<(
        String,
        String,
        String,
        String,
    )>,
    Json(request): Json<SovdCapabilityRequest>,
) -> Result<StatusCode, StatusCode> {
    if entity_collection != "components" {
        return Err(StatusCode::NOT_FOUND);
    }

    let _ = resolve_ecu_url(&state, &id)?;
    let manager = get_execution_manager(&state).await?;
    let execution = manager
        .get_execution(&execution_id)
        .await
        .map_err(manager_error_status)?;

    if execution.ecu_id != id || execution.operation_id != operation_id {
        return Err(StatusCode::NOT_FOUND);
    }

    match request.capability.as_str() {
        "stop" => {
            manager
                .request_stop(&execution_id)
                .await
                .map_err(manager_error_status)?;
            Ok(StatusCode::NO_CONTENT)
        }
        "status" => Ok(StatusCode::NO_CONTENT),
        "execute" | "freeze" | "reset" => Err(StatusCode::NOT_IMPLEMENTED),
        _ => Err(StatusCode::BAD_REQUEST),
    }
}
