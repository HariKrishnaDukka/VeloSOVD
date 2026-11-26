//! Raw UDS diagnostic passthrough handler.

use super::*;
use crate::uds_policy::*;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

pub(crate) async fn diagnostic_request(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<UdsDiagnosticRequest>,
) -> Result<Json<UdsDiagnosticResponse>, StatusCode> {
    let normalized_payload = validate_payload_hex(&request.payload_hex)?;
    check_raw_uds_policy(&normalized_payload)?;

    let normalized_request = UdsDiagnosticRequest {
        payload_hex: normalized_payload,
    };

    let url = resolve_ecu_url(&state, &id)?;
    let response = state
        .client
        .post(format!("{url}/diagnostics/requests"))
        .json(&normalized_request)
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    if !response.status().is_success() {
        return Err(
            StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY)
        );
    }

    let result = response
        .json::<UdsDiagnosticResponse>()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    Ok(Json(result))
}
