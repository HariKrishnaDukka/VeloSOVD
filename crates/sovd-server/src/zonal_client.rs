//! Central-to-zonal HTTPS/mTLS client and the zonal clear-faults call.

use crate::{api_error::*, models::*};
use axum::http::StatusCode;
use reqwest::Client;
use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ClearFaultError {
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
    pub(crate) fn generic_error(&self) -> SovdGenericError {
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

    pub(crate) fn http_status(&self) -> StatusCode {
        match self {
            Self::Timeout => StatusCode::GATEWAY_TIMEOUT,
            Self::QueueFull | Self::DispatcherUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Upstream { status, .. } => *status,
            _ => StatusCode::BAD_GATEWAY,
        }
    }

    pub(crate) fn execution_message(&self) -> String {
        serde_json::to_string(&self.generic_error())
            .unwrap_or_else(|_| "operation-failed: Clear-fault operation failed".to_string())
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

pub(crate) fn decode_uds_response_hex(input: &str) -> Result<Vec<u8>, ClearFaultError> {
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

pub(crate) fn validate_clear_fault_response(
    response_hex: Option<&str>,
) -> Result<(), ClearFaultError> {
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

pub(crate) async fn invoke_clear_faults(
    client: &Client,
    url: String,
) -> Result<(), ClearFaultError> {
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

/// Build the Central-to-Zonal HTTPS client.
///
/// Trust is restricted to the configured CA. Central presents its
/// client certificate when connecting to a zonal server.
pub(crate) fn build_zonal_mtls_client() -> Result<Client, Box<dyn std::error::Error + Send + Sync>>
{
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
