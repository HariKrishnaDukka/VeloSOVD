pub mod execution;
mod tls_config;

use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::execution::{
    executor::{
        ExecutionContext, ExecutionError, ExecutionFuture, ExecutionOutput, OperationExecutor,
    },
    manager::{ExecutionManager, ExecutionManagerError},
    model::{ExecutionCapabilities, ExecutionState, OperationDefinition},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use diagnostic_common::{DiagnosticFault, DiagnosticValue, EcuInfo, OperationResult};
use reqwest::Client;
use tokio::sync::Mutex;
use tokio::sync::OnceCell;
use vehicle_topology::{Ecu, VehicleTopology};

const EXECUTION_HISTORY_CAPACITY: usize = 128;

#[derive(Clone)]
struct AppState {
    client: Client,
    topology: VehicleTopology,
    zonal_http_port: u16,
    zonal_scheme: &'static str,
    next_execution_id: Arc<AtomicU64>,
    executions: Arc<Mutex<VecDeque<OperationExecution>>>,
    execution_manager: Arc<OnceCell<ExecutionManager>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct UdsDiagnosticRequest {
    payload_hex: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct UdsDiagnosticResponse {
    ecu_id: String,
    request_hex: String,
    response_hex: Option<String>,
}

#[derive(Debug, Serialize)]
struct ZoneHealth {
    id: String,
    name: String,
    online: bool,
    detail: String,
}

#[derive(Debug, Serialize)]
struct VehicleHealth {
    vehicle_id: String,
    vehicle_name: String,
    overall: String,
    zones: Vec<ZoneHealth>,
}

#[derive(Debug, Serialize)]
struct ResourceDescriptor {
    id: String,
    path: String,
    kind: String,
    description: String,
    methods: Vec<String>,
    simulated: bool,
}

#[derive(Debug, Serialize)]
struct OperationDescriptor {
    id: String,
    name: String,
    method: String,
    execution_path: String,
    description: String,
    supports_execution: bool,
}

#[derive(Debug, Clone, Serialize)]
struct OperationExecution {
    execution_id: String,
    ecu_id: String,
    operation: String,
    status: String,
    success: bool,
    message: String,
    created_at_unix_ms: u64,
}

#[derive(Debug, Deserialize)]
struct FaultQuery {
    active: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct SovdStartExecutionRequest {
    timeout: Option<u64>,
    parameters: Option<serde_json::Value>,
    proximity_response: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
struct SovdGenericError {
    error_code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    vendor_code: Option<String>,
}

#[derive(Debug)]
struct SovdApiError {
    status: StatusCode,
    error: SovdGenericError,
}

impl IntoResponse for SovdApiError {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(self.error)).into_response()
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ClearFaultError {
    NegativeResponse(u8),
    EmptyResponse,
    TruncatedResponse,
    UnexpectedResponseLength(usize),
    WrongResponseSid(u8),
    MalformedResponse(String),
    Timeout,
    QueueFull,
    DispatcherUnavailable,
    ResponseUnavailable,
    Transport(String),
    Upstream {
        status: StatusCode,
        error: SovdGenericError,
    },
}

impl ClearFaultError {
    fn generic_error(&self) -> SovdGenericError {
        let (error_code, message, vendor_code) = match self {
            Self::NegativeResponse(nrc) => (
                "uds-negative-response",
                format!("ECU rejected ClearDiagnosticInformation with NRC 0x{nrc:02X}"),
                Some(format!("UDS_NRC_0x{nrc:02X}")),
            ),
            Self::EmptyResponse => (
                "uds-empty-response",
                "ECU returned no UDS response for ClearDiagnosticInformation".to_string(),
                None,
            ),
            Self::TruncatedResponse => (
                "uds-truncated-response",
                "ECU returned a truncated UDS negative response".to_string(),
                None,
            ),
            Self::UnexpectedResponseLength(length) => (
                "uds-unexpected-response-length",
                format!("Expected exactly one UDS response byte 0x54, received {length} bytes"),
                None,
            ),
            Self::WrongResponseSid(sid) => (
                "uds-wrong-response-sid",
                format!("Expected positive response SID 0x54, received 0x{sid:02X}"),
                None,
            ),
            Self::MalformedResponse(detail) => (
                "uds-malformed-response",
                format!("Zonal service returned an invalid UDS response: {detail}"),
                None,
            ),
            Self::Timeout => (
                "uds-timeout",
                "Timed out waiting for the ECU UDS clear response".to_string(),
                Some("UDS_RESPONSE_TIMEOUT".to_string()),
            ),
            Self::QueueFull => (
                "uds-queue-full",
                "The ECU UDS request queue is full".to_string(),
                Some("UDS_QUEUE_FULL".to_string()),
            ),
            Self::DispatcherUnavailable => (
                "uds-dispatcher-unavailable",
                "The ECU UDS dispatcher is unavailable".to_string(),
                Some("UDS_ACTOR_UNAVAILABLE".to_string()),
            ),
            Self::ResponseUnavailable => (
                "uds-response-unavailable",
                "The ECU UDS response channel closed before a response was received".to_string(),
                Some("UDS_RESPONSE_UNAVAILABLE".to_string()),
            ),
            Self::Transport(detail) => (
                "uds-transport-error",
                format!("Could not communicate with the zonal ECU service: {detail}"),
                None,
            ),
            Self::Upstream { error, .. } => return error.clone(),
        };

        SovdGenericError {
            error_code: error_code.to_string(),
            message,
            vendor_code,
        }
    }

    fn http_status(&self) -> StatusCode {
        match self {
            Self::Timeout => StatusCode::GATEWAY_TIMEOUT,
            Self::QueueFull | Self::DispatcherUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Upstream { status, .. } => *status,
            _ => StatusCode::BAD_GATEWAY,
        }
    }

    fn execution_message(&self) -> String {
        serde_json::to_string(&self.generic_error())
            .unwrap_or_else(|_| "operation-failed: Clear-fault operation failed".to_string())
    }

    fn display_message(&self) -> String {
        self.generic_error().message
    }
}

impl From<ClearFaultError> for SovdApiError {
    fn from(error: ClearFaultError) -> Self {
        Self {
            status: error.http_status(),
            error: error.generic_error(),
        }
    }
}

fn decode_uds_response_hex(input: &str) -> Result<Vec<u8>, ClearFaultError> {
    if input.is_empty() {
        return Err(ClearFaultError::EmptyResponse);
    }
    if !input.len().is_multiple_of(2) {
        return Err(ClearFaultError::MalformedResponse(
            "response_hex has an odd number of characters".to_string(),
        ));
    }

    let mut bytes = Vec::with_capacity(input.len() / 2);
    for pair in input.as_bytes().as_chunks::<2>().0 {
        let pair = std::str::from_utf8(pair)
            .map_err(|error| ClearFaultError::MalformedResponse(error.to_string()))?;
        let byte = u8::from_str_radix(pair, 16)
            .map_err(|error| ClearFaultError::MalformedResponse(error.to_string()))?;
        bytes.push(byte);
    }
    Ok(bytes)
}

fn validate_clear_fault_response(response_hex: Option<&str>) -> Result<(), ClearFaultError> {
    let response_hex = response_hex.ok_or(ClearFaultError::EmptyResponse)?;
    let bytes = decode_uds_response_hex(response_hex)?;

    if bytes.as_slice() == [0x54] {
        return Ok(());
    }
    if bytes.len() == 2 && bytes[0] == 0x7F && bytes[1] == 0x14 {
        return Err(ClearFaultError::TruncatedResponse);
    }
    if bytes.len() == 3 && bytes[0] == 0x7F && bytes[1] == 0x14 {
        return Err(ClearFaultError::NegativeResponse(bytes[2]));
    }
    if bytes.len() >= 2 && bytes[0] == 0x7F && bytes[1] != 0x14 {
        return Err(ClearFaultError::WrongResponseSid(bytes[1]));
    }
    if bytes.len() == 1 {
        return Err(ClearFaultError::WrongResponseSid(bytes[0]));
    }
    Err(ClearFaultError::UnexpectedResponseLength(bytes.len()))
}

async fn invoke_clear_faults(client: &Client, url: String) -> Result<(), ClearFaultError> {
    let response = client.post(url).send().await.map_err(|error| {
        if error.is_timeout() {
            ClearFaultError::Timeout
        } else {
            ClearFaultError::Transport(error.to_string())
        }
    })?;

    if !response.status().is_success() {
        let status =
            StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        if let Ok(error) = response.json::<SovdGenericError>().await {
            return Err(match error.error_code.as_str() {
                "queue-full" => ClearFaultError::QueueFull,
                "dispatcher-unavailable" => ClearFaultError::DispatcherUnavailable,
                "response-unavailable" => ClearFaultError::ResponseUnavailable,
                "timeout" => ClearFaultError::Timeout,
                _ => ClearFaultError::Upstream { status, error },
            });
        }
        return Err(match status {
            StatusCode::GATEWAY_TIMEOUT => ClearFaultError::Timeout,
            StatusCode::SERVICE_UNAVAILABLE => ClearFaultError::QueueFull,
            _ => ClearFaultError::Transport(format!("zonal HTTP status {}", status.as_u16())),
        });
    }

    let result = response
        .json::<UdsDiagnosticResponse>()
        .await
        .map_err(|error| ClearFaultError::MalformedResponse(error.to_string()))?;

    validate_clear_fault_response(result.response_hex.as_deref())
}

fn generic_error_from_execution_message(message: &str) -> SovdGenericError {
    serde_json::from_str::<SovdGenericError>(message).unwrap_or_else(|_| SovdGenericError {
        error_code: "operation-failed".to_string(),
        message: message.to_string(),
        vendor_code: None,
    })
}

#[derive(Debug, Serialize)]
struct SovdStartExecutionResponse {
    id: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct SovdExecutionIds {
    items: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SovdExecutionStatus {
    status: String,
    capability: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Vec<SovdExecutionError>>,
}

#[derive(Debug, Serialize)]
struct SovdExecutionError {
    path: String,
    error: SovdGenericError,
}

#[derive(Debug, Deserialize)]
struct SovdCapabilityRequest {
    capability: String,
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn validate_payload_hex(input: &str) -> Result<String, StatusCode> {
    if input.len() > 8192 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    let compact: String = input
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect();

    if compact.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    if compact.len() > 8192 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    let (byte_pairs, remainder) = compact.as_bytes().as_chunks::<2>();

    if !remainder.is_empty()
        || !byte_pairs
            .iter()
            .all(|pair| pair.iter().all(u8::is_ascii_hexdigit))
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    Ok(compact)
}

/// UDS services the raw passthrough endpoint may forward.
///
/// Deny by default: only read-only diagnostics and session keep-alive are
/// allowed. Write (0x2E), reset (0x11), routines (0x31), security access
/// (0x27), authentication (0x29), communication control (0x28),
/// DTC setting (0x85), clear DTC (0x14) and all download / upload / transfer
/// services (0x34-0x38) are refused here; they must go through dedicated,
/// authorized SOVD resources.
const RAW_UDS_ALLOWED_SIDS: [u8; 4] = [0x10, 0x19, 0x22, 0x3E];

/// DiagnosticSessionControl sub-functions allowed through the raw endpoint.
/// Programming session (0x02) and vendor sessions are refused.
const RAW_UDS_ALLOWED_SESSIONS: [u8; 2] = [0x01, 0x03];

/// Apply the raw UDS passthrough policy to a validated, normalized hex payload.
fn check_raw_uds_policy(normalized_hex: &str) -> Result<(), StatusCode> {
    let sid = u8::from_str_radix(normalized_hex.get(0..2).ok_or(StatusCode::BAD_REQUEST)?, 16)
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    if !RAW_UDS_ALLOWED_SIDS.contains(&sid) {
        return Err(StatusCode::FORBIDDEN);
    }

    if sid == 0x10 {
        let sub = u8::from_str_radix(normalized_hex.get(2..4).ok_or(StatusCode::BAD_REQUEST)?, 16)
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        // Bit 7 is suppressPosRspMsgIndicationBit; policy applies to the session type.
        if !RAW_UDS_ALLOWED_SESSIONS.contains(&(sub & 0x7F)) {
            return Err(StatusCode::FORBIDDEN);
        }
    }

    Ok(())
}

fn resolve_ecu_url(state: &AppState, ecu_id: &str) -> Result<String, StatusCode> {
    let ecu: &Ecu = state
        .topology
        .ecus
        .iter()
        .find(|ecu| ecu.id == ecu_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let zone = state
        .topology
        .zones
        .iter()
        .find(|zone| zone.id == ecu.zone)
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(format!(
        "{}://{}:{}/ecus/{}",
        state.zonal_scheme, zone.zonal_ip, state.zonal_http_port, ecu.id
    ))
}

async fn get_topology(State(state): State<AppState>) -> Json<serde_json::Value> {
    let topology = &state.topology;

    Json(serde_json::json!({
        "vehicle": {
            "id": topology.vehicle.id,
            "name": topology.vehicle.name
        },
        "central": {
            "namespace": topology.central.namespace,
            "sovd_bind": topology.central.sovd_bind,
            "sovd_port": topology.central.sovd_port
        },
        "zones": topology.zones.iter().map(|zone| serde_json::json!({
            "id": zone.id,
            "name": zone.name,
            "namespace": zone.namespace,
            "can_interface": zone.can_interface,
            "central_ip": zone.central_ip,
            "zonal_ip": zone.zonal_ip,
            "doip_port": zone.doip_port,
            "doip_logical_address": zone.doip_logical_address
        })).collect::<Vec<_>>(),
        "ecus": topology.ecus.iter().map(|ecu| serde_json::json!({
            "id": ecu.id,
            "name": ecu.name,
            "domain": ecu.domain,
            "zone": ecu.zone,
            "doip_logical_address": ecu.doip_logical_address,
            "can_interface": ecu.can_interface,
            "can_request_id": ecu.can_request_id,
            "can_response_id": ecu.can_response_id
        })).collect::<Vec<_>>()
    }))
}

async fn health(State(state): State<AppState>) -> Json<VehicleHealth> {
    let mut zones = Vec::with_capacity(state.topology.zones.len());

    for zone in &state.topology.zones {
        let endpoint = format!(
            "{}://{}:{}/ecus",
            state.zonal_scheme, zone.zonal_ip, state.zonal_http_port
        );
        let probe = state.client.get(&endpoint).send().await;

        let (online, detail) = match probe {
            Ok(response) if response.status().is_success() => (
                true,
                format!(
                    "{} {}",
                    state.zonal_scheme.to_ascii_uppercase(),
                    response.status().as_u16()
                ),
            ),
            Ok(response) => (
                false,
                format!(
                    "{} {}",
                    state.zonal_scheme.to_ascii_uppercase(),
                    response.status().as_u16()
                ),
            ),
            Err(error) => (false, error.to_string()),
        };

        zones.push(ZoneHealth {
            id: zone.id.clone(),
            name: zone.name.clone(),
            online,
            detail,
        });
    }

    let overall = if zones.iter().all(|zone| zone.online) {
        "online"
    } else if zones.iter().any(|zone| zone.online) {
        "degraded"
    } else {
        "offline"
    };

    Json(VehicleHealth {
        vehicle_id: state.topology.vehicle.id.clone(),
        vehicle_name: state.topology.vehicle.name.clone(),
        overall: overall.to_string(),
        zones,
    })
}

async fn list_ecus(State(state): State<AppState>) -> Result<Json<Vec<EcuInfo>>, StatusCode> {
    let mut result = Vec::new();

    for zone in &state.topology.zones {
        let url = format!(
            "{}://{}:{}/ecus",
            state.zonal_scheme, zone.zonal_ip, state.zonal_http_port
        );

        let response = state
            .client
            .get(url)
            .send()
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?
            .error_for_status()
            .map_err(|_| StatusCode::BAD_GATEWAY)?;

        let mut zone_ecus = response
            .json::<Vec<EcuInfo>>()
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?;

        result.append(&mut zone_ecus);
    }

    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Json(result))
}

async fn ecu_info(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<EcuInfo>, StatusCode> {
    let url = resolve_ecu_url(&state, &id)?;

    let response = state
        .client
        .get(url)
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?
        .error_for_status()
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let info = response
        .json::<EcuInfo>()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    Ok(Json(info))
}

async fn ecu_resources(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ResourceDescriptor>>, StatusCode> {
    let _ = resolve_ecu_url(&state, &id)?;
    let base = format!("/api/ecus/{id}");

    let resources = vec![
        ResourceDescriptor {
            id: "vehicle-speed".to_string(),
            path: format!("{base}/data/vehicle-speed"),
            kind: "data".to_string(),
            description: "Vehicle speed value in km/h".to_string(),
            methods: vec!["GET".to_string()],
            simulated: true,
        },
        ResourceDescriptor {
            id: "faults".to_string(),
            path: format!("{base}/diagnostics/faults"),
            kind: "faults".to_string(),
            description: "Diagnostic trouble codes reported by this ECU".to_string(),
            methods: vec!["GET".to_string(), "DELETE".to_string()],
            simulated: true,
        },
        ResourceDescriptor {
            id: "uds-diagnostic-request".to_string(),
            path: format!("{base}/diagnostics/requests"),
            kind: "diagnostic-request".to_string(),
            description: "Submit a raw UDS diagnostic request as hexadecimal bytes".to_string(),
            methods: vec!["POST".to_string()],
            simulated: true,
        },
        ResourceDescriptor {
            id: "clear-faults".to_string(),
            path: format!("{base}/diagnostics/operations/clear-faults"),
            kind: "operation".to_string(),
            description: "Clear the ECU's simulated diagnostic fault memory".to_string(),
            methods: vec!["POST".to_string()],
            simulated: true,
        },
    ];

    Ok(Json(resources))
}

async fn ecu_operations(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<OperationDescriptor>>, StatusCode> {
    let _ = resolve_ecu_url(&state, &id)?;

    Ok(Json(vec![OperationDescriptor {
        id: "clear-faults".to_string(),
        name: "Clear fault memory".to_string(),
        method: "POST".to_string(),
        execution_path: format!("/api/ecus/{id}/operations/clear-faults/executions"),
        description: "Clear all stored diagnostic faults for this ECU".to_string(),
        supports_execution: true,
    }]))
}

async fn ecu_operation(
    State(state): State<AppState>,
    Path((id, operation_id)): Path<(String, String)>,
) -> Result<Json<OperationDescriptor>, StatusCode> {
    let _ = resolve_ecu_url(&state, &id)?;

    match operation_id.as_str() {
        "clear-faults" => Ok(Json(OperationDescriptor {
            id: "clear-faults".to_string(),
            name: "Clear fault memory".to_string(),
            method: "POST".to_string(),
            execution_path: format!("/api/ecus/{id}/operations/clear-faults/executions"),
            description: "Clear all stored diagnostic faults for this ECU".to_string(),
            supports_execution: true,
        })),
        _ => Err(StatusCode::NOT_FOUND),
    }
}

async fn vehicle_speed(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DiagnosticValue>, StatusCode> {
    let url = resolve_ecu_url(&state, &id)?;

    let response = state
        .client
        .get(format!("{url}/data/vehicle-speed"))
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?
        .error_for_status()
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let value = response
        .json::<DiagnosticValue>()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    Ok(Json(value))
}

async fn faults(
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

async fn fault_by_code(
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

async fn delete_faults(
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

async fn delete_fault(
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

async fn clear_faults(
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

async fn run_clear_faults(
    state: &AppState,
    id: String,
    operation_id: &str,
) -> Result<OperationExecution, StatusCode> {
    if operation_id != "clear-faults" {
        return Err(StatusCode::NOT_FOUND);
    }

    let url = resolve_ecu_url(state, &id)?;

    // Do not hold the execution-history lock during network I/O. Once the
    // operation is admitted, record both positive and negative outcomes.
    let result = invoke_clear_faults(
        &state.client,
        format!("{url}/diagnostics/operations/clear-faults"),
    )
    .await;

    let (success, message) = match result {
        Ok(()) => (true, format!("{id} fault memory cleared")),
        Err(error) => (false, error.display_message()),
    };

    let sequence = state.next_execution_id.fetch_add(1, Ordering::Relaxed);

    // Use the validated API operation ID, not the backend's
    // differently formatted "clear_faults" identifier.
    let execution = OperationExecution {
        execution_id: format!("exec-{sequence:016}"),
        ecu_id: id,
        operation: operation_id.to_string(),
        status: if success {
            "completed".to_string()
        } else {
            "failed".to_string()
        },
        success,
        message,
        created_at_unix_ms: unix_time_ms(),
    };

    {
        let mut history = state.executions.lock().await;

        if history.len() >= EXECUTION_HISTORY_CAPACITY {
            history.pop_front();
        }

        history.push_back(execution.clone());
    }

    Ok(execution)
}

async fn execute_clear_faults(
    State(state): State<AppState>,
    Path((id, operation_id)): Path<(String, String)>,
) -> Result<Json<OperationExecution>, StatusCode> {
    let execution = run_clear_faults(&state, id, &operation_id).await?;
    Ok(Json(execution))
}

/// Executor for the demonstrator's existing zonal clear-faults operation.
/// Stop/freeze/reset are intentionally not advertised: cancelling this HTTP
/// future cannot prove that the remote ECU stopped processing the request.
struct ClearFaultsExecutor {
    client: Client,
    topology: VehicleTopology,
    zonal_http_port: u16,
    zonal_scheme: &'static str,
}

impl OperationExecutor for ClearFaultsExecutor {
    fn capabilities(&self) -> ExecutionCapabilities {
        ExecutionCapabilities {
            execute: true,
            stop: false,
            freeze: false,
            reset: false,
            status: true,
        }
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

async fn get_execution_manager(state: &AppState) -> Result<&ExecutionManager, StatusCode> {
    state
        .execution_manager
        .get_or_try_init(|| async {
            let manager = ExecutionManager::new_with_limits(EXECUTION_HISTORY_CAPACITY, 8)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            let capabilities = ExecutionCapabilities {
                execute: true,
                stop: false,
                freeze: false,
                reset: false,
                status: true,
            };

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

fn execution_manager_status(state: ExecutionState) -> &'static str {
    match state {
        ExecutionState::Created
        | ExecutionState::Running
        | ExecutionState::StopRequested
        | ExecutionState::Frozen => "running",
        ExecutionState::Completed | ExecutionState::Stopped => "completed",
        ExecutionState::Failed => "failed",
    }
}

fn manager_error_status(error: ExecutionManagerError) -> StatusCode {
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

async fn sovd_start_operation_execution(
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

async fn list_operation_executions(
    State(state): State<AppState>,
    Path((id, operation_id)): Path<(String, String)>,
) -> Result<Json<Vec<OperationExecution>>, StatusCode> {
    let _ = resolve_ecu_url(&state, &id)?;
    let history = state.executions.lock().await;

    let executions = history
        .iter()
        .filter(|execution| execution.ecu_id == id && execution.operation == operation_id)
        .cloned()
        .collect();

    Ok(Json(executions))
}

async fn get_operation_execution(
    State(state): State<AppState>,
    Path((id, operation_id, execution_id)): Path<(String, String, String)>,
) -> Result<Json<OperationExecution>, StatusCode> {
    let _ = resolve_ecu_url(&state, &id)?;
    let history = state.executions.lock().await;

    let execution = history
        .iter()
        .find(|execution| {
            execution.ecu_id == id
                && execution.operation == operation_id
                && execution.execution_id == execution_id
        })
        .cloned()
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(execution))
}

async fn sovd_list_operation_executions(
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

async fn sovd_get_operation_execution(
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

async fn sovd_delete_operation_execution(
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

async fn sovd_put_execution_capability(
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

async fn diagnostic_request(
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

fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/topology", get(get_topology))
        .route("/api/health", get(health))
        .route("/api/ecus", get(list_ecus))
        .route("/api/ecus/{id}", get(ecu_info))
        .route("/api/ecus/{id}/resources", get(ecu_resources))
        .route("/api/ecus/{id}/operations", get(ecu_operations))
        .route(
            "/api/ecus/{id}/operation-descriptions/{operation_id}",
            get(ecu_operation),
        )
        .route("/api/ecus/{id}/data/vehicle-speed", get(vehicle_speed))
        .route(
            "/api/ecus/{id}/diagnostics/faults",
            get(faults).delete(delete_faults),
        )
        .route(
            "/api/ecus/{id}/diagnostics/faults/{fault_code}",
            get(fault_by_code).delete(delete_fault),
        )
        .route(
            "/api/ecus/{id}/diagnostics/operations/clear-faults",
            post(clear_faults),
        )
        .route(
            "/api/ecus/{id}/operations/{operation_id}/executions",
            post(execute_clear_faults).get(list_operation_executions),
        )
        .route(
            "/api/ecus/{id}/operations/{operation_id}/executions/{execution_id}",
            get(get_operation_execution),
        )
        .route(
            "/sovd/v1/{entity_collection}/{id}/operations/{operation_id}/executions",
            post(sovd_start_operation_execution).get(sovd_list_operation_executions),
        )
        .route(
            "/sovd/v1/{entity_collection}/{id}/operations/{operation_id}/executions/{execution_id}",
            get(sovd_get_operation_execution)
                .delete(sovd_delete_operation_execution)
                .put(sovd_put_execution_capability),
        )
        .route(
            "/api/ecus/{id}/diagnostics/requests",
            post(diagnostic_request),
        )
        .with_state(state)
}

/// Build the Central-to-Zonal HTTPS client.
///
/// Trust is restricted to the configured CA. Central presents its
/// client certificate when connecting to a zonal server.
fn build_zonal_mtls_client() -> Result<Client, Box<dyn std::error::Error + Send + Sync>> {
    // Central authenticates zonal servers using the dedicated Server CA.
    let ca_path = std::env::var("SDV_TLS_SERVER_CA_CERT")
        .unwrap_or_else(|_| "pki/dev/server-ca/ca.crt".to_string());

    // Central presents its Client CA-issued identity to each zonal server.
    let cert_path = std::env::var("SDV_TLS_CLIENT_CERT")
        .unwrap_or_else(|_| "pki/dev/central/central-client.crt".to_string());

    let key_path = std::env::var("SDV_TLS_CLIENT_KEY")
        .unwrap_or_else(|_| "pki/dev/central/central-client.key".to_string());

    let ca_pem = std::fs::read(&ca_path)?;
    let client_cert_pem = std::fs::read(&cert_path)?;
    let client_key_pem = std::fs::read(&key_path)?;

    let ca_certificate = reqwest::Certificate::from_pem(&ca_pem)?;

    // Reqwest expects the certificate and private key in one PEM identity.
    // Concatenate in memory; do not create another private-key file.
    let mut identity_pem = client_cert_pem;
    identity_pem.push(b'\n');
    identity_pem.extend_from_slice(&client_key_pem);

    let identity = reqwest::Identity::from_pem(&identity_pem)?;

    Client::builder()
        .tls_built_in_root_certs(false)
        .add_root_certificate(ca_certificate)
        .identity(identity)
        .min_tls_version(reqwest::tls::Version::TLS_1_3)
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(Into::into)
}

#[tokio::main]
async fn main() {
    let topology_path = std::env::var("SDV_TOPOLOGY")
        .unwrap_or_else(|_| "platform/config/vehicle-topology.toml".to_string());

    let topology = VehicleTopology::load(&topology_path)
        .unwrap_or_else(|error| panic!("Topology load failed: {error}"));

    let client = build_zonal_mtls_client().unwrap_or_else(|error| {
        panic!("Cannot initialize Central-to-Zonal mTLS client: {error}");
    });

    let state = AppState {
        client,
        topology,
        zonal_http_port: 9001,
        zonal_scheme: "https",
        next_execution_id: Arc::new(AtomicU64::new(1)),
        executions: Arc::new(Mutex::new(VecDeque::with_capacity(
            EXECUTION_HISTORY_CAPACITY,
        ))),
        execution_manager: Arc::new(OnceCell::new()),
    };

    let app = build_router(state.clone());

    let bind_address = format!(
        "{}:{}",
        state.topology.central.sovd_bind, state.topology.central.sovd_port
    );

    let client_ca_path = std::env::var("SDV_TLS_CLIENT_CA_CERT")
        .unwrap_or_else(|_| "pki/dev/client-ca/ca.crt".to_string());

    let server_cert_path = std::env::var("SDV_TLS_SERVER_CERT")
        .unwrap_or_else(|_| "pki/dev/central/central-server.crt".to_string());

    let server_key_path = std::env::var("SDV_TLS_SERVER_KEY")
        .unwrap_or_else(|_| "pki/dev/central/central-server.key".to_string());

    let tls_config = tls_config::load_server_config(
        std::path::Path::new(&client_ca_path),
        std::path::Path::new(&server_cert_path),
        std::path::Path::new(&server_key_path),
    )
    .unwrap_or_else(|error| {
        panic!("[SOVD] Central HTTPS/mTLS configuration failed: {error}");
    });

    let listener = tokio::net::TcpListener::bind(&bind_address)
        .await
        .unwrap_or_else(|error| {
            panic!("Cannot bind central SOVD server at {bind_address}: {error}")
        });

    let std_listener = listener
        .into_std()
        .expect("Cannot convert Central TCP listener");

    let tls_server = axum_server::from_tcp_rustls(std_listener, tls_config)
        .expect("Cannot initialize Central TLS server");

    println!(
        "[SOVD] Vehicle: {} ({})",
        state.topology.vehicle.name, state.topology.vehicle.id
    );
    println!("[SOVD] Listening at https://{bind_address} (TLS 1.3, mTLS required)");
    println!("[SOVD] Configured zones: {}", state.topology.zones.len());
    println!("[SOVD] Configured ECUs: {}", state.topology.ecus.len());
    println!(
        "[SOVD] Operation history capacity: {}",
        EXECUTION_HISTORY_CAPACITY
    );

    tls_server
        .serve(app.into_make_service())
        .await
        .expect("Central HTTPS server failed");
}

#[cfg(test)]
mod payload_validation_tests {
    use super::validate_payload_hex;
    use axum::http::StatusCode;

    #[test]
    fn accepts_normal_hex_payload() {
        assert_eq!(validate_payload_hex("22F190"), Ok("22F190".to_string()));
    }

    #[test]
    fn removes_ascii_whitespace() {
        assert_eq!(validate_payload_hex("22 F1 90"), Ok("22F190".to_string()));
    }

    #[test]
    fn accepts_mixed_case_hex() {
        assert_eq!(validate_payload_hex("22 f1 F0"), Ok("22f1F0".to_string()));
    }

    #[test]
    fn rejects_empty_payload() {
        assert_eq!(validate_payload_hex(""), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn rejects_whitespace_only_payload() {
        assert_eq!(validate_payload_hex("   "), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn rejects_odd_length_payload() {
        assert_eq!(validate_payload_hex("22F19"), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn rejects_non_hexadecimal_characters() {
        assert_eq!(validate_payload_hex("22G190"), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn accepts_maximum_payload() {
        let payload = "AA".repeat(4096);
        assert_eq!(validate_payload_hex(&payload), Ok(payload));
    }

    #[test]
    fn rejects_input_over_limit() {
        let payload = " ".repeat(8193);
        assert_eq!(
            validate_payload_hex(&payload),
            Err(StatusCode::PAYLOAD_TOO_LARGE)
        );
    }
}

#[cfg(test)]
mod payload_forwarding_integration_tests {
    use super::*;
    use axum::{extract::State, routing::post};
    use serde_json::Value;
    use std::{
        path::PathBuf,
        sync::{
            Arc, Mutex as StdMutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    type MockEcuState = (Arc<AtomicUsize>, Arc<StdMutex<Option<String>>>);

    async fn mock_ecu(
        State((request_count, received_payload)): State<MockEcuState>,
        Json(request): Json<UdsDiagnosticRequest>,
    ) -> Json<UdsDiagnosticResponse> {
        request_count.fetch_add(1, Ordering::SeqCst);

        let forwarded_payload = request.payload_hex;
        *received_payload
            .lock()
            .expect("Payload capture mutex must not be poisoned") = Some(forwarded_payload.clone());

        Json(UdsDiagnosticResponse {
            ecu_id: "ECM".to_string(),
            request_hex: forwarded_payload,
            response_hex: Some("62F190".to_string()),
        })
    }

    #[tokio::test]
    async fn operation_execution_routes_filter_by_operation_and_ecu() {
        let topology_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../platform/config/vehicle-topology.toml");
        let topology = VehicleTopology::load(&topology_path).expect("Test topology must load");

        let history = Arc::new(Mutex::new(VecDeque::new()));
        {
            let mut entries = history.lock().await;

            entries.push_back(OperationExecution {
                execution_id: "exec-clear-001".to_string(),
                ecu_id: "ECM".to_string(),
                operation: "clear-faults".to_string(),
                status: "completed".to_string(),
                success: true,
                message: "Faults cleared".to_string(),
                created_at_unix_ms: 1000,
            });

            entries.push_back(OperationExecution {
                execution_id: "exec-read-001".to_string(),
                ecu_id: "ECM".to_string(),
                operation: "read-status".to_string(),
                status: "completed".to_string(),
                success: true,
                message: "Status read".to_string(),
                created_at_unix_ms: 2000,
            });

            entries.push_back(OperationExecution {
                execution_id: "exec-clear-failed-001".to_string(),
                ecu_id: "ECM".to_string(),
                operation: "clear-faults".to_string(),
                status: "failed".to_string(),
                success: false,
                message: "Fault memory clear failed".to_string(),
                created_at_unix_ms: 2500,
            });

            entries.push_back(OperationExecution {
                execution_id: "exec-bcm-001".to_string(),
                ecu_id: "BCM".to_string(),
                operation: "clear-faults".to_string(),
                status: "completed".to_string(),
                success: true,
                message: "BCM faults cleared".to_string(),
                created_at_unix_ms: 3000,
            });
        }

        let state = AppState {
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("HTTP client must initialize"),
            topology,
            zonal_http_port: 9001,
            zonal_scheme: "http",
            next_execution_id: Arc::new(AtomicU64::new(1)),
            executions: history,
            execution_manager: Arc::new(OnceCell::new()),
        };

        let app = build_router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Central test listener must bind");
        let address = listener
            .local_addr()
            .expect("Central test address must be available");

        let server_task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("Central test server failed");
        });

        let client = Client::new();
        let base_url = format!("http://{address}");

        // Only ECM executions belonging to clear-faults should be listed.
        let response = client
            .get(format!(
                "{base_url}/api/ecus/ECM/operations/clear-faults/executions"
            ))
            .send()
            .await
            .expect("Execution list request must complete");

        assert_eq!(response.status(), StatusCode::OK);
        let listed: serde_json::Value = response
            .json()
            .await
            .expect("Execution list response must be valid JSON");

        let listed_items = listed
            .as_array()
            .expect("Execution list must be a JSON array");

        assert_eq!(listed_items.len(), 2);
        assert_eq!(listed_items[0]["execution_id"], "exec-clear-001");
        assert_eq!(listed_items[0]["operation"], "clear-faults");
        assert_eq!(listed_items[0]["ecu_id"], "ECM");
        assert_eq!(listed_items[1]["execution_id"], "exec-clear-failed-001");
        assert_eq!(listed_items[1]["success"], false);

        // The same execution must be retrievable under its owning operation.
        let response = client
            .get(format!(
                "{base_url}/api/ecus/ECM/operations/clear-faults/executions/exec-clear-001"
            ))
            .send()
            .await
            .expect("Execution detail request must complete");

        assert_eq!(response.status(), StatusCode::OK);
        let detail: serde_json::Value = response
            .json()
            .await
            .expect("Execution detail response must be valid JSON");

        assert_eq!(detail["execution_id"], "exec-clear-001");
        assert_eq!(detail["operation"], "clear-faults");

        // A different operation ID must not expose the same execution.
        let response = client
            .get(format!(
                "{base_url}/api/ecus/ECM/operations/read-status/executions/exec-clear-001"
            ))
            .send()
            .await
            .expect("Mismatched operation request must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        server_task.abort();
    }

    #[tokio::test]
    async fn retrieves_individual_operation_description() {
        let topology_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../platform/config/vehicle-topology.toml");

        let topology = VehicleTopology::load(&topology_path).expect("Test topology must load");

        let state = AppState {
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("HTTP client must initialize"),
            topology,
            zonal_http_port: 9001,
            zonal_scheme: "http",
            next_execution_id: Arc::new(AtomicU64::new(1)),
            executions: Arc::new(Mutex::new(VecDeque::with_capacity(
                EXECUTION_HISTORY_CAPACITY,
            ))),
            execution_manager: Arc::new(OnceCell::new()),
        };

        let app = build_router(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Central test listener must bind");

        let address = listener
            .local_addr()
            .expect("Central test address must be available");

        let central_task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("Central test server failed");
        });

        let client = Client::new();

        // Test 1: Retrieve the existing clear-faults operation.
        let response = client
            .get(format!(
                "http://{address}/api/ecus/ECM/operation-descriptions/clear-faults"
            ))
            .send()
            .await
            .expect("Operation description request must complete");

        assert_eq!(response.status(), StatusCode::OK);

        let body: Value = response
            .json()
            .await
            .expect("Operation description must be valid JSON");

        assert_eq!(body["id"], "clear-faults");
        assert_eq!(body["name"], "Clear fault memory");
        assert_eq!(body["method"], "POST");
        assert_eq!(body["supports_execution"], true);
        assert_eq!(
            body["execution_path"],
            "/api/ecus/ECM/operations/clear-faults/executions"
        );

        // Test 2: An unknown operation must return 404.
        let response = client
            .get(format!(
                "http://{address}/api/ecus/ECM/operation-descriptions/unknown-operation"
            ))
            .send()
            .await
            .expect("Unknown operation request must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // Test 3: An unknown ECU must return 404.
        let response = client
            .get(format!(
                "http://{address}/api/ecus/UNKNOWN/operation-descriptions/clear-faults"
            ))
            .send()
            .await
            .expect("Unknown ECU request must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        central_task.abort();
    }

    #[tokio::test]
    async fn forwards_normalized_payload_and_rejects_invalid_requests() {
        // Load a private in-memory copy of the project topology.
        let topology_path: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../platform/config/vehicle-topology.toml");

        let mut topology = VehicleTopology::load(&topology_path).expect("Test topology must load");

        // Redirect FRONT zone to the local mock ECU.
        let front_zone = topology
            .zones
            .iter_mut()
            .find(|zone| zone.id == "FRONT")
            .expect("FRONT zone must exist");

        front_zone.zonal_ip = "127.0.0.1".to_string();

        // Start mock ECU on an operating-system-assigned port.
        let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Mock ECU listener must bind");

        let mock_port = mock_listener
            .local_addr()
            .expect("Mock ECU address must be available")
            .port();

        let request_count = Arc::new(AtomicUsize::new(0));
        let received_payload = Arc::new(StdMutex::new(None));

        let mock_app = Router::new()
            .route("/ecus/ECM/diagnostics/requests", post(mock_ecu))
            .with_state((Arc::clone(&request_count), Arc::clone(&received_payload)));

        let mock_task = tokio::spawn(async move {
            axum::serve(mock_listener, mock_app)
                .await
                .expect("Mock ECU server failed");
        });

        // Construct the actual central SOVD application.
        let state = AppState {
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("HTTP client must initialize"),
            topology,
            zonal_http_port: mock_port,
            zonal_scheme: "http",
            next_execution_id: Arc::new(AtomicU64::new(1)),
            executions: Arc::new(Mutex::new(VecDeque::with_capacity(
                EXECUTION_HISTORY_CAPACITY,
            ))),
            execution_manager: Arc::new(OnceCell::new()),
        };

        let app = build_router(state);

        let central_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Central test listener must bind");

        let central_address = central_listener
            .local_addr()
            .expect("Central test address must be available");

        let central_task = tokio::spawn(async move {
            axum::serve(central_listener, app)
                .await
                .expect("Central test server failed");
        });

        let client = Client::new();
        let endpoint = format!(
            "http://{}/api/ecus/ECM/diagnostics/requests",
            central_address
        );

        // Test 1: Whitespace is removed before forwarding.
        let response = client
            .post(&endpoint)
            .json(&serde_json::json!({
                "payload_hex": "22 f1 90"
            }))
            .send()
            .await
            .expect("Central diagnostic request must complete");

        assert_eq!(response.status(), StatusCode::OK);

        let response_body: Value = response
            .json()
            .await
            .expect("Central response must be valid JSON");

        assert_eq!(response_body["ecu_id"], "ECM");
        assert_eq!(response_body["request_hex"], "22f190");
        assert_eq!(response_body["response_hex"], "62F190");

        assert_eq!(
            request_count.load(Ordering::SeqCst),
            1,
            "Mock ECU must receive exactly one valid request"
        );

        assert_eq!(
            received_payload
                .lock()
                .expect("Payload capture mutex must not be poisoned")
                .as_deref(),
            Some("22f190"),
            "Mock ECU must receive the normalized hexadecimal payload"
        );

        // Test 2: Invalid hexadecimal must be rejected before forwarding.
        let response = client
            .post(&endpoint)
            .json(&serde_json::json!({
                "payload_hex": "22G190"
            }))
            .send()
            .await
            .expect("Invalid diagnostic request must complete");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        assert_eq!(
            request_count.load(Ordering::SeqCst),
            1,
            "Invalid payload must not reach the mock ECU"
        );

        // Test 3: Unknown ECU must be rejected before forwarding.
        let unknown_endpoint = format!(
            "http://{}/api/ecus/UNKNOWN/diagnostics/requests",
            central_address
        );

        let response = client
            .post(&unknown_endpoint)
            .json(&serde_json::json!({
                "payload_hex": "22F190"
            }))
            .send()
            .await
            .expect("Unknown ECU request must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        assert_eq!(
            request_count.load(Ordering::SeqCst),
            1,
            "Unknown ECU request must not reach the mock ECU"
        );

        central_task.abort();
        mock_task.abort();
    }

    #[tokio::test]
    async fn sovd_post_executes_operation_and_records_execution() {
        use axum::routing::post;

        type MockDtcStore = Arc<StdMutex<Vec<String>>>;

        async fn mock_clear_faults(
            State(dtcs): State<MockDtcStore>,
        ) -> Json<UdsDiagnosticResponse> {
            dtcs.lock()
                .expect("Mock DTC store lock must succeed")
                .clear();

            Json(UdsDiagnosticResponse {
                ecu_id: "ECM".to_string(),
                request_hex: "14FFFFFF".to_string(),
                response_hex: Some("54".to_string()),
            })
        }

        let topology_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../platform/config/vehicle-topology.toml");

        let mut topology = VehicleTopology::load(&topology_path).expect("Test topology must load");

        for zone in &mut topology.zones {
            zone.zonal_ip = "127.0.0.1".to_string();
        }

        let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Mock zonal listener must bind");

        let mock_port = mock_listener
            .local_addr()
            .expect("Mock zonal address must be available")
            .port();

        let seeded_dtcs = Arc::new(StdMutex::new(vec!["P0128".to_string()]));
        let mock_app = Router::new()
            .route(
                "/ecus/ECM/diagnostics/operations/clear-faults",
                post(mock_clear_faults),
            )
            .with_state(Arc::clone(&seeded_dtcs));

        let mock_task = tokio::spawn(async move {
            axum::serve(mock_listener, mock_app)
                .await
                .expect("Mock zonal server failed");
        });

        let state = AppState {
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("HTTP client must initialize"),
            topology,
            zonal_http_port: mock_port,
            zonal_scheme: "http",
            next_execution_id: Arc::new(AtomicU64::new(1)),
            executions: Arc::new(Mutex::new(VecDeque::with_capacity(
                EXECUTION_HISTORY_CAPACITY,
            ))),
            execution_manager: Arc::new(OnceCell::new()),
        };

        let app = build_router(state);

        let central_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Central listener must bind");

        let central_address = central_listener
            .local_addr()
            .expect("Central address must be available");

        let central_task = tokio::spawn(async move {
            axum::serve(central_listener, app)
                .await
                .expect("Central server failed");
        });

        let client = Client::new();
        let base_url = format!("http://{central_address}");

        // Execute clear-faults through the SOVD-facing endpoint.
        let response = client
            .post(format!(
                "{base_url}/sovd/v1/components/ECM/operations/clear-faults/executions"
            ))
            .json(&serde_json::json!({}))
            .send()
            .await
            .expect("SOVD execution request must complete");

        assert_eq!(response.status(), StatusCode::ACCEPTED);

        let response_location = response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);

        let start_result: Value = response
            .json()
            .await
            .expect("SOVD asynchronous response must be valid JSON");

        let execution_id = start_result["id"]
            .as_str()
            .expect("Asynchronous response must contain an execution ID")
            .to_string();

        assert_eq!(start_result["status"], "running");

        let expected_location =
            format!("/sovd/v1/components/ECM/operations/clear-faults/executions/{execution_id}");
        assert_eq!(
            response_location.as_deref(),
            Some(expected_location.as_str())
        );

        // Verify that the execution was stored under the route operation ID.
        let response = client
            .get(format!(
                "{base_url}/sovd/v1/components/ECM/operations/clear-faults/executions"
            ))
            .send()
            .await
            .expect("SOVD execution collection request must complete");

        assert_eq!(response.status(), StatusCode::OK);

        let collection: Value = response
            .json()
            .await
            .expect("SOVD collection must be valid JSON");

        let items = collection["items"]
            .as_array()
            .expect("SOVD collection items must be an array");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].as_str(), Some(execution_id.as_str()));
        assert!(execution_id.starts_with("exec-"));

        // Poll the execution resource because the zonal operation runs
        // asynchronously. Do not assume completion immediately after POST.
        let mut final_status = Value::Null;
        let mut reached_terminal_state = false;

        for _ in 0..100 {
            let response = client
                .get(format!(
                    "{base_url}/sovd/v1/components/ECM/operations/clear-faults/executions/{execution_id}"
                ))
                .send()
                .await
                .expect("SOVD execution status request must complete");

            assert_eq!(response.status(), StatusCode::OK);

            final_status = response
                .json()
                .await
                .expect("SOVD execution status must be valid JSON");

            let status = final_status["status"].as_str().unwrap_or_default();
            if status == "completed" || status == "failed" {
                reached_terminal_state = true;
                break;
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert!(
            reached_terminal_state,
            "Execution did not reach a terminal state: {final_status}"
        );
        assert_eq!(final_status["status"], "completed");
        assert_eq!(final_status["capability"], "execute");
        assert!(final_status.get("error").is_none());
        assert!(
            seeded_dtcs.lock().unwrap().is_empty(),
            "The seeded DTC must be cleared by the accepted UDS positive response"
        );

        // Unsupported operation IDs must not be forwarded.
        let response = client
            .post(format!(
                "{base_url}/sovd/v1/components/ECM/operations/unknown-operation/executions"
            ))
            .json(&serde_json::json!({}))
            .send()
            .await
            .expect("Unknown operation request must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // Unsupported entity collections must be rejected.
        let response = client
            .post(format!(
                "{base_url}/sovd/v1/unknown/ECM/operations/clear-faults/executions"
            ))
            .json(&serde_json::json!({}))
            .send()
            .await
            .expect("Unknown entity collection request must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        central_task.abort();
        mock_task.abort();
    }

    #[tokio::test]
    async fn list_ecus_uses_configured_zonal_http_port() {
        use axum::routing::get;

        async fn mock_ecus() -> Json<Vec<diagnostic_common::EcuInfo>> {
            Json(vec![diagnostic_common::EcuInfo {
                id: "ECM".to_string(),
                name: "Engine Control Module".to_string(),
                domain: "powertrain".to_string(),
                status: "online".to_string(),
            }])
        }

        let topology_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../platform/config/vehicle-topology.toml");

        let mut topology = VehicleTopology::load(&topology_path).expect("Test topology must load");

        // let front_zone = topology
        //     .zones
        //     .iter_mut()
        //     .find(|zone| zone.id == "FRONT")
        //     .expect("FRONT zone must exist");

        for zone in &mut topology.zones {
            zone.zonal_ip = "127.0.0.1".to_string();
        }

        let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Mock listener must bind");

        let mock_port = mock_listener.local_addr().unwrap().port();

        let mock_app = Router::new().route("/ecus", get(mock_ecus));

        let mock_task = tokio::spawn(async move {
            axum::serve(mock_listener, mock_app)
                .await
                .expect("Mock server failed");
        });

        let state = AppState {
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("HTTP client must initialize"),
            topology,
            zonal_http_port: mock_port,
            zonal_scheme: "http",
            next_execution_id: Arc::new(AtomicU64::new(1)),
            executions: Arc::new(Mutex::new(VecDeque::with_capacity(
                EXECUTION_HISTORY_CAPACITY,
            ))),
            execution_manager: Arc::new(OnceCell::new()),
        };

        let app = build_router(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Central listener must bind");

        let address = listener.local_addr().unwrap();

        let central_task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("Central server failed");
        });

        let response = Client::new()
            .get(format!("http://{address}/api/ecus"))
            .send()
            .await
            .expect("ECU discovery request must complete");

        assert_eq!(response.status(), StatusCode::OK);

        let ecus: Vec<diagnostic_common::EcuInfo> = response
            .json()
            .await
            .expect("ECU discovery response must be valid JSON");

        assert!(
            ecus.iter().any(|ecu| ecu.id == "ECM"),
            "ECU discovery must receive the mock ECU through the configured port"
        );

        central_task.abort();
        mock_task.abort();
    }

    #[tokio::test]
    async fn health_uses_configured_zonal_http_port() {
        use axum::routing::get;

        async fn mock_ecus() -> Json<Vec<diagnostic_common::EcuInfo>> {
            Json(Vec::new())
        }

        let topology_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../platform/config/vehicle-topology.toml");

        let mut topology = VehicleTopology::load(&topology_path).expect("Test topology must load");

        let front_zone = topology
            .zones
            .iter_mut()
            .find(|zone| zone.id == "FRONT")
            .expect("FRONT zone must exist");

        front_zone.zonal_ip = "127.0.0.1".to_string();

        let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Mock listener must bind");

        let mock_port = mock_listener.local_addr().unwrap().port();

        let mock_app = Router::new().route("/ecus", get(mock_ecus));

        let mock_task = tokio::spawn(async move {
            axum::serve(mock_listener, mock_app)
                .await
                .expect("Mock server failed");
        });

        let state = AppState {
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("HTTP client must initialize"),
            topology,
            zonal_http_port: mock_port,
            zonal_scheme: "http",
            next_execution_id: Arc::new(AtomicU64::new(1)),
            executions: Arc::new(Mutex::new(VecDeque::with_capacity(
                EXECUTION_HISTORY_CAPACITY,
            ))),
            execution_manager: Arc::new(OnceCell::new()),
        };

        let app = build_router(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Central listener must bind");

        let address = listener.local_addr().unwrap();

        let central_task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("Central server failed");
        });

        let response = Client::new()
            .get(format!("http://{address}/api/health"))
            .send()
            .await
            .expect("Health request must complete");

        assert_eq!(response.status(), StatusCode::OK);

        let body: Value = response
            .json()
            .await
            .expect("Health response must be valid JSON");

        assert!(
            body.to_string().contains("online"),
            "Health must report the reachable FRONT zone as online"
        );

        central_task.abort();
        mock_task.abort();
    }

    #[tokio::test]
    async fn retrieves_individual_fault_from_zonal_server() {
        use axum::routing::get;

        async fn mock_faults(
            Path(ecu_id): Path<String>,
        ) -> Result<Json<Vec<DiagnosticFault>>, StatusCode> {
            if ecu_id != "ECM" {
                return Err(StatusCode::NOT_FOUND);
            }

            Ok(Json(vec![
                DiagnosticFault {
                    code: "P0128".to_string(),
                    description: "Coolant thermostat temperature below threshold".to_string(),
                    active: true,
                    status: 0x09,
                },
                DiagnosticFault {
                    code: "P0300".to_string(),
                    description: "Random or multiple cylinder misfire detected".to_string(),
                    active: false,
                    status: 0x08,
                },
            ]))
        }
        async fn mock_fault(
            Path((ecu_id, fault_code)): Path<(String, String)>,
        ) -> Result<Json<DiagnosticFault>, StatusCode> {
            if ecu_id != "ECM" || fault_code != "P0128" {
                return Err(StatusCode::NOT_FOUND);
            }

            Ok(Json(DiagnosticFault {
                code: "P0128".to_string(),
                description: "Coolant thermostat temperature below threshold".to_string(),
                active: true,
                status: 0x09,
            }))
        }

        async fn mock_delete_faults(Path(ecu_id): Path<String>) -> Result<StatusCode, StatusCode> {
            if ecu_id != "ECM" {
                return Err(StatusCode::NOT_FOUND);
            }

            Ok(StatusCode::NO_CONTENT)
        }

        async fn mock_delete_fault(
            Path((ecu_id, fault_code)): Path<(String, String)>,
        ) -> Result<StatusCode, StatusCode> {
            if ecu_id != "ECM" || fault_code != "P0128" {
                return Err(StatusCode::NOT_FOUND);
            }

            Ok(StatusCode::NO_CONTENT)
        }

        let topology_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../platform/config/vehicle-topology.toml");

        let mut topology = VehicleTopology::load(&topology_path).expect("Test topology must load");

        for zone in &mut topology.zones {
            zone.zonal_ip = "127.0.0.1".to_string();
        }

        let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Mock listener must bind");

        let mock_port = mock_listener
            .local_addr()
            .expect("Mock address must be available")
            .port();

        let mock_app = Router::new()
            .route(
                "/ecus/{id}/diagnostics/faults",
                get(mock_faults).delete(mock_delete_faults),
            )
            .route(
                "/ecus/{id}/diagnostics/faults/{fault_code}",
                get(mock_fault).delete(mock_delete_fault),
            );

        let mock_task = tokio::spawn(async move {
            axum::serve(mock_listener, mock_app)
                .await
                .expect("Mock server failed");
        });

        let state = AppState {
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("HTTP client must initialize"),
            topology,
            zonal_http_port: mock_port,
            zonal_scheme: "http",
            next_execution_id: Arc::new(AtomicU64::new(1)),
            executions: Arc::new(Mutex::new(VecDeque::with_capacity(
                EXECUTION_HISTORY_CAPACITY,
            ))),
            execution_manager: Arc::new(OnceCell::new()),
        };

        let app = build_router(state);

        let central_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Central listener must bind");

        let central_address = central_listener
            .local_addr()
            .expect("Central address must be available");

        let central_task = tokio::spawn(async move {
            axum::serve(central_listener, app)
                .await
                .expect("Central server failed");
        });

        let client = Client::new();

        // Test 1: Existing fault must be returned successfully.
        let endpoint = format!(
            "http://{}/api/ecus/ECM/diagnostics/faults/P0128",
            central_address
        );

        let response = client
            .get(&endpoint)
            .send()
            .await
            .expect("Fault request must complete");

        assert_eq!(response.status(), StatusCode::OK);

        let fault: DiagnosticFault = response
            .json()
            .await
            .expect("Fault response must be valid JSON");

        assert_eq!(fault.code, "P0128");
        assert_eq!(
            fault.description,
            "Coolant thermostat temperature below threshold"
        );
        assert!(fault.active);

        // Test 2: No filter must return both faults.
        let fault_list_endpoint =
            format!("http://{}/api/ecus/ECM/diagnostics/faults", central_address);

        let response = client
            .get(&fault_list_endpoint)
            .send()
            .await
            .expect("Fault-list request must complete");

        assert_eq!(response.status(), StatusCode::OK);

        let faults: Vec<DiagnosticFault> = response
            .json()
            .await
            .expect("Fault-list response must be valid JSON");

        assert_eq!(faults.len(), 2);

        // Test 3: active=true must return only active faults.
        let response = client
            .get(format!("{fault_list_endpoint}?active=true"))
            .send()
            .await
            .expect("Active fault filter request must complete");

        assert_eq!(response.status(), StatusCode::OK);

        let faults: Vec<DiagnosticFault> = response
            .json()
            .await
            .expect("Active fault response must be valid JSON");

        assert_eq!(faults.len(), 1);
        assert_eq!(faults[0].code, "P0128");
        assert!(faults[0].active);

        // Test 4: active=false must return only inactive faults.
        let response = client
            .get(format!("{fault_list_endpoint}?active=false"))
            .send()
            .await
            .expect("Inactive fault filter request must complete");

        assert_eq!(response.status(), StatusCode::OK);

        let faults: Vec<DiagnosticFault> = response
            .json()
            .await
            .expect("Inactive fault response must be valid JSON");

        assert_eq!(faults.len(), 1);
        assert_eq!(faults[0].code, "P0300");
        assert!(!faults[0].active);

        // Test 2: Unknown fault must return 404.
        let unknown_fault_endpoint = format!(
            "http://{}/api/ecus/ECM/diagnostics/faults/P9999",
            central_address
        );

        let response = client
            .get(&unknown_fault_endpoint)
            .send()
            .await
            .expect("Unknown fault request must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // Test 3: Unknown ECU must return 404.
        let unknown_ecu_endpoint = format!(
            "http://{}/api/ecus/UNKNOWN/diagnostics/faults/P0128",
            central_address
        );

        let response = client
            .get(&unknown_ecu_endpoint)
            .send()
            .await
            .expect("Unknown ECU request must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // Test 4: Delete an existing individual fault.
        let delete_fault_endpoint = format!(
            "http://{}/api/ecus/ECM/diagnostics/faults/P0128",
            central_address
        );

        let response = client
            .delete(&delete_fault_endpoint)
            .send()
            .await
            .expect("Individual fault deletion must complete");

        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Test 5: Deleting an unknown fault must return 404.
        let unknown_delete_endpoint = format!(
            "http://{}/api/ecus/ECM/diagnostics/faults/P9999",
            central_address
        );

        let response = client
            .delete(&unknown_delete_endpoint)
            .send()
            .await
            .expect("Unknown fault deletion must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // Test 6: Delete the complete fault collection.
        let delete_collection_endpoint =
            format!("http://{}/api/ecus/ECM/diagnostics/faults", central_address);

        let response = client
            .delete(&delete_collection_endpoint)
            .send()
            .await
            .expect("Fault collection deletion must complete");

        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Test 7: Unknown ECU collection deletion must return 404.
        let unknown_collection_endpoint = format!(
            "http://{}/api/ecus/UNKNOWN/diagnostics/faults",
            central_address
        );

        let response = client
            .delete(&unknown_collection_endpoint)
            .send()
            .await
            .expect("Unknown ECU deletion must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        central_task.abort();
        mock_task.abort();
    }
}

#[cfg(test)]
mod clear_fault_response_tests {
    use super::*;

    #[test]
    fn accepts_only_exact_one_byte_positive_response() {
        assert_eq!(validate_clear_fault_response(Some("54")), Ok(()));
    }

    #[test]
    fn maps_negative_response_code() {
        assert!(matches!(
            validate_clear_fault_response(Some("7F1431")),
            Err(ClearFaultError::NegativeResponse(0x31))
        ));
    }

    #[test]
    fn distinguishes_empty_truncated_wrong_sid_and_long_responses() {
        assert!(matches!(
            validate_clear_fault_response(None),
            Err(ClearFaultError::EmptyResponse)
        ));
        assert!(matches!(
            validate_clear_fault_response(Some("")),
            Err(ClearFaultError::EmptyResponse)
        ));
        assert!(matches!(
            validate_clear_fault_response(Some("7F14")),
            Err(ClearFaultError::TruncatedResponse)
        ));
        assert!(matches!(
            validate_clear_fault_response(Some("55")),
            Err(ClearFaultError::WrongResponseSid(0x55))
        ));
        assert!(matches!(
            validate_clear_fault_response(Some("7F2231")),
            Err(ClearFaultError::WrongResponseSid(0x22))
        ));
        assert!(matches!(
            validate_clear_fault_response(Some("5400")),
            Err(ClearFaultError::UnexpectedResponseLength(2))
        ));
    }

    #[test]
    fn rejects_malformed_response_hex() {
        assert!(matches!(
            validate_clear_fault_response(Some("5Z")),
            Err(ClearFaultError::MalformedResponse(_))
        ));
        assert!(matches!(
            validate_clear_fault_response(Some("5")),
            Err(ClearFaultError::MalformedResponse(_))
        ));
    }

    #[test]
    fn generic_error_json_uses_kebab_case_and_optional_vendor_code() {
        let negative = ClearFaultError::NegativeResponse(0x31).generic_error();
        let negative_json = serde_json::to_value(negative).unwrap();
        assert_eq!(negative_json["error_code"], "uds-negative-response");
        assert_eq!(negative_json["vendor_code"], "UDS_NRC_0x31");
        assert!(negative_json["message"].as_str().is_some());

        let empty = ClearFaultError::EmptyResponse.generic_error();
        let empty_json = serde_json::to_value(empty).unwrap();
        assert_eq!(empty_json["error_code"], "uds-empty-response");
        assert!(empty_json.get("vendor_code").is_none());
    }
}

#[cfg(test)]
mod clear_fault_history_tests {
    use super::*;
    use axum::routing::post;
    use serde_json::Value;
    use std::path::PathBuf;

    async fn mock_negative_clear() -> Json<UdsDiagnosticResponse> {
        Json(UdsDiagnosticResponse {
            ecu_id: "ECM".to_string(),
            request_hex: "14FFFFFF".to_string(),
            response_hex: Some("7F1431".to_string()),
        })
    }

    #[tokio::test]
    async fn negative_uds_response_is_recorded_as_failed_execution() {
        let topology_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../platform/config/vehicle-topology.toml");
        let mut topology = VehicleTopology::load(&topology_path).expect("Test topology must load");

        for zone in &mut topology.zones {
            zone.zonal_ip = "127.0.0.1".to_string();
        }

        let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Mock zonal listener must bind");
        let mock_port = mock_listener.local_addr().unwrap().port();
        let mock_app = Router::new().route(
            "/ecus/ECM/diagnostics/operations/clear-faults",
            post(mock_negative_clear),
        );
        let mock_task = tokio::spawn(async move {
            axum::serve(mock_listener, mock_app)
                .await
                .expect("Mock zonal server failed");
        });

        let state = AppState {
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("HTTP client must initialize"),
            topology,
            zonal_http_port: mock_port,
            zonal_scheme: "http",
            next_execution_id: Arc::new(AtomicU64::new(1)),
            executions: Arc::new(Mutex::new(VecDeque::with_capacity(
                EXECUTION_HISTORY_CAPACITY,
            ))),
            execution_manager: Arc::new(OnceCell::new()),
        };
        let app = build_router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Central listener must bind");
        let address = listener.local_addr().unwrap();
        let central_task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("Central server failed");
        });

        let client = Client::new();
        let base = format!("http://{address}");
        let response = client
            .post(format!(
                "{base}/api/ecus/ECM/operations/clear-faults/executions"
            ))
            .send()
            .await
            .expect("Clear execution request must complete");
        assert_eq!(response.status(), StatusCode::OK);

        let execution: Value = response.json().await.unwrap();
        assert_eq!(execution["success"], false);
        assert_eq!(execution["status"], "failed");
        assert!(execution["message"].as_str().unwrap().contains("NRC 0x31"));

        let history_response = client
            .get(format!(
                "{base}/api/ecus/ECM/operations/clear-faults/executions"
            ))
            .send()
            .await
            .expect("Execution history request must complete");
        assert_eq!(history_response.status(), StatusCode::OK);
        let history: Value = history_response.json().await.unwrap();
        assert_eq!(history.as_array().unwrap().len(), 1);
        assert_eq!(history[0]["success"], false);
        assert_eq!(history[0]["status"], "failed");

        // The SOVD-facing execution adapter must expose the same failure as
        // a generic error object with a kebab-case error_code.
        let start = client
            .post(format!(
                "{base}/sovd/v1/components/ECM/operations/clear-faults/executions"
            ))
            .json(&serde_json::json!({}))
            .send()
            .await
            .expect("SOVD negative-response execution must start");
        assert_eq!(start.status(), StatusCode::ACCEPTED);
        let start_body: Value = start.json().await.unwrap();
        let sovd_execution_id = start_body["id"].as_str().unwrap();

        let mut sovd_status = Value::Null;
        for _ in 0..100 {
            let response = client
                .get(format!(
                    "{base}/sovd/v1/components/ECM/operations/clear-faults/executions/{sovd_execution_id}"
                ))
                .send()
                .await
                .expect("SOVD execution status request must complete");
            sovd_status = response.json().await.unwrap();
            if sovd_status["status"] == "failed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert_eq!(sovd_status["status"], "failed");
        assert_eq!(
            sovd_status["error"][0]["error"]["error_code"],
            "uds-negative-response"
        );
        assert_eq!(
            sovd_status["error"][0]["error"]["vendor_code"],
            "UDS_NRC_0x31"
        );

        central_task.abort();
        mock_task.abort();
    }
}

#[cfg(test)]
mod raw_uds_policy_tests {
    use super::check_raw_uds_policy;
    use axum::http::StatusCode;

    #[test]
    fn allows_read_only_services() {
        for payload in ["22F190", "1902FF", "3E00", "3E80", "1001", "1003", "1083"] {
            assert_eq!(check_raw_uds_policy(payload), Ok(()), "{payload}");
        }
    }

    #[test]
    fn refuses_write_reset_routine_security_and_transfer() {
        for payload in [
            "2EF19001",
            "1101",
            "3101FF00",
            "2701",
            "2901",
            "280300",
            "8502",
            "14FFFFFF",
            "3400441000000000000100",
            "3601",
            "37",
        ] {
            assert_eq!(
                check_raw_uds_policy(payload),
                Err(StatusCode::FORBIDDEN),
                "{payload}"
            );
        }
    }

    #[test]
    fn refuses_programming_and_unknown_sessions() {
        for payload in ["1002", "1082", "1040", "1060"] {
            assert_eq!(
                check_raw_uds_policy(payload),
                Err(StatusCode::FORBIDDEN),
                "{payload}"
            );
        }
    }

    #[test]
    fn session_control_without_subfunction_is_bad_request() {
        assert_eq!(check_raw_uds_policy("10"), Err(StatusCode::BAD_REQUEST));
    }
}
