//! SOVD GenericError types used in handler responses.

use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct SovdGenericError {
    pub(crate) error_code: String,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) vendor_code: Option<String>,
}

#[derive(Debug)]
pub(crate) struct SovdApiError {
    pub(crate) status: StatusCode,
    pub(crate) error: SovdGenericError,
}

impl IntoResponse for SovdApiError {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(self.error)).into_response()
    }
}

pub(crate) fn generic_error_from_execution_message(message: &str) -> SovdGenericError {
    serde_json::from_str::<SovdGenericError>(message).unwrap_or_else(|_| SovdGenericError {
        error_code: "operation-failed".to_string(),
        message: message.to_string(),
        vendor_code: None,
    })
}
