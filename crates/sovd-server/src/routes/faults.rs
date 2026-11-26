//! Fault (DTC) read, delete and clear handlers.

use super::*;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use diagnostic_common::{DiagnosticFault, OperationResult};

pub(crate) async fn faults(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<FaultQuery>,
) -> Result<Json<Vec<DiagnosticFault>>, StatusCode> {
    let url = resolve_ecu_url(&state, &id)?;

    let response = state
        .client
        .get(format!("{url}/diagnostics/faults"))
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?
        .error_for_status()
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let all_faults = response
        .json::<Vec<DiagnosticFault>>()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let filtered_faults = all_faults
        .into_iter()
        .filter(|fault| query.active.is_none_or(|active| fault.active == active))
        .collect();

    Ok(Json(filtered_faults))
}

pub(crate) async fn fault_by_code(
    State(state): State<AppState>,
    Path((id, fault_code)): Path<(String, String)>,
) -> Result<Json<DiagnosticFault>, StatusCode> {
    let url = resolve_ecu_url(&state, &id)?;

    let response = state
        .client
        .get(format!("{url}/diagnostics/faults/{fault_code}"))
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let status = response.status();

    if !status.is_success() {
        return Err(StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY));
    }

    let fault = response
        .json::<DiagnosticFault>()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    Ok(Json(fault))
}

pub(crate) async fn delete_faults(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, SovdApiError> {
    let url = resolve_ecu_url(&state, &id).map_err(|status| SovdApiError {
        status,
        error: SovdGenericError {
            error_code: "not-found".to_string(),
            message: format!("ECU '{id}' is not configured"),
            vendor_code: None,
        },
    })?;

    let response = state
        .client
        .delete(format!("{url}/diagnostics/faults"))
        .send()
        .await
        .map_err(|error| SovdApiError::from(ClearFaultError::Transport(error.to_string())))?;

    let status = response.status();

    if !status.is_success() {
        let status = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let error = response
            .json::<SovdGenericError>()
            .await
            .unwrap_or_else(|_| SovdGenericError {
                error_code: "uds-transport-error".to_string(),
                message: format!(
                    "Zonal fault collection DELETE returned HTTP {}",
                    status.as_u16()
                ),
                vendor_code: None,
            });
        return Err(SovdApiError { status, error });
    }

    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn delete_fault(
    State(state): State<AppState>,
    Path((id, fault_code)): Path<(String, String)>,
) -> Result<StatusCode, StatusCode> {
    let url = resolve_ecu_url(&state, &id)?;

    let response = state
        .client
        .delete(format!("{url}/diagnostics/faults/{fault_code}"))
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let status = response.status();

    if !status.is_success() {
        return Err(StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY));
    }

    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn clear_faults(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<OperationResult>, SovdApiError> {
    let url = resolve_ecu_url(&state, &id).map_err(|status| SovdApiError {
        status,
        error: SovdGenericError {
            error_code: "not-found".to_string(),
            message: format!("ECU '{id}' is not configured"),
            vendor_code: None,
        },
    })?;

    invoke_clear_faults(
        &state.client,
        format!("{url}/diagnostics/operations/clear-faults"),
    )
    .await
    .map_err(SovdApiError::from)?;

    Ok(Json(OperationResult {
        operation: "clear_faults".to_string(),
        success: true,
        message: format!("{id} fault memory cleared"),
    }))
}
