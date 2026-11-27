//! HTTP request/response models of the central API (`/api` and `/sovd/v1`).

use crate::api_error::*;
use crate::execution::model::ExecutionCapabilities;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct UdsDiagnosticRequest {
    pub(crate) payload_hex: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct UdsDiagnosticResponse {
    pub(crate) ecu_id: String,
    pub(crate) request_hex: String,
    pub(crate) response_hex: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ZoneHealth {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) online: bool,
    pub(crate) detail: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct VehicleHealth {
    pub(crate) vehicle_id: String,
    pub(crate) vehicle_name: String,
    pub(crate) overall: String,
    pub(crate) zones: Vec<ZoneHealth>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ResourceDescriptor {
    pub(crate) id: String,
    pub(crate) path: String,
    pub(crate) kind: String,
    pub(crate) description: String,
    pub(crate) methods: Vec<String>,
    pub(crate) simulated: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct OperationDescriptor {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) method: String,
    pub(crate) execution_path: String,
    pub(crate) description: String,
    pub(crate) supports_execution: bool,
    /// What the operation's executor implements (execute, stop, freeze, reset, status).
    pub(crate) capabilities: ExecutionCapabilities,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct OperationExecution {
    pub(crate) execution_id: String,
    pub(crate) ecu_id: String,
    pub(crate) operation: String,
    pub(crate) status: String,
    pub(crate) success: bool,
    pub(crate) message: String,
    pub(crate) created_at_unix_ms: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FaultQuery {
    pub(crate) active: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SovdStartExecutionRequest {
    pub(crate) timeout: Option<u64>,
    pub(crate) parameters: Option<serde_json::Value>,
    pub(crate) proximity_response: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SovdStartExecutionResponse {
    pub(crate) id: String,
    pub(crate) status: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SovdExecutionIds {
    pub(crate) items: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SovdExecutionStatus {
    pub(crate) status: String,
    pub(crate) capability: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<Vec<SovdExecutionError>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SovdExecutionError {
    pub(crate) path: String,
    pub(crate) error: SovdGenericError,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SovdCapabilityRequest {
    pub(crate) capability: String,
}
