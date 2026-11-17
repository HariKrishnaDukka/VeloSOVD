//! Transport-independent DoIP diagnostic routing.

use std::{collections::HashMap, error::Error, fmt, sync::Arc};

use crate::{
    connection::RoutingConnection,
    diagnostic::{DiagnosticMessage, DiagnosticPayloadError},
};

/// Failure categories returned by a UDS request handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UdsDispatchFailure {
    QueueFull,
    ActorUnavailable,
    ResponseUnavailable,
    ResponseTimeout,
}

/// Adapter interface implemented by the application using this gateway.
pub trait UdsRequestHandler: Send + Sync {
    fn process_uds(&self, payload: &[u8]) -> Result<Option<Vec<u8>>, UdsDispatchFailure>;
}

/// A successfully routed diagnostic response.
///
/// Addresses are reversed for the response:
/// ECU logical address becomes source; tester becomes target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedDiagnosticResponse {
    pub source_address: u16,
    pub target_address: u16,
    pub uds_payload: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouterError {
    RoutingNotActive,
    TesterAddressMismatch,
    UnknownTargetAddress,
    DuplicateTargetAddress,
    InvalidDiagnosticPayload(DiagnosticPayloadError),
    Dispatch(UdsDispatchFailure),
}

impl fmt::Display for RouterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RoutingNotActive => write!(f, "DoIP routing is not active"),
            Self::TesterAddressMismatch => {
                write!(f, "Diagnostic source does not match activated tester")
            }
            Self::UnknownTargetAddress => {
                write!(f, "Unknown target ECU logical address")
            }
            Self::DuplicateTargetAddress => {
                write!(f, "Duplicate ECU logical address")
            }
            Self::InvalidDiagnosticPayload(error) => {
                write!(f, "Invalid diagnostic payload: {error}")
            }
            Self::Dispatch(error) => write!(f, "UDS dispatch failed: {error:?}"),
        }
    }
}

impl Error for RouterError {}

/// Routes DoIP diagnostic messages to ECU-specific UDS handlers.
#[derive(Default)]
pub struct DiagnosticRouter {
    handlers: HashMap<u16, Arc<dyn UdsRequestHandler>>,
}

impl DiagnosticRouter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register one handler for an ECU logical address.
    /// Existing registrations are never silently overwritten.
    pub fn register(
        &mut self,
        logical_address: u16,
        handler: Arc<dyn UdsRequestHandler>,
    ) -> Result<(), RouterError> {
        if self.handlers.contains_key(&logical_address) {
            return Err(RouterError::DuplicateTargetAddress);
        }

        self.handlers.insert(logical_address, handler);
        Ok(())
    }

    pub fn contains_target(&self, logical_address: u16) -> bool {
        self.handlers.contains_key(&logical_address)
    }

    pub fn target_count(&self) -> usize {
        self.handlers.len()
    }

    /// Validate and dispatch one DoIP diagnostic payload.
    ///
    /// Call this method from a blocking worker when the handler performs
    /// synchronous work. Do not block a Tokio runtime worker.
    pub fn route(
        &self,
        connection: &RoutingConnection,
        payload: &[u8],
    ) -> Result<RoutedDiagnosticResponse, RouterError> {
        if !connection.permits_diagnostic_messages() {
            return Err(RouterError::RoutingNotActive);
        }

        let message =
            DiagnosticMessage::decode(payload).map_err(RouterError::InvalidDiagnosticPayload)?;

        let tester_address = connection
            .tester_logical_address()
            .ok_or(RouterError::RoutingNotActive)?;

        if message.source_address != tester_address {
            return Err(RouterError::TesterAddressMismatch);
        }

        let handler = self
            .handlers
            .get(&message.target_address)
            .ok_or(RouterError::UnknownTargetAddress)?;

        let uds_response = handler
            .process_uds(message.uds_payload)
            .map_err(RouterError::Dispatch)?;

        Ok(RoutedDiagnosticResponse {
            source_address: message.target_address,
            target_address: message.source_address,
            uds_payload: uds_response,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::RoutingActivationRequest;

    struct EchoHandler;

    impl UdsRequestHandler for EchoHandler {
        fn process_uds(&self, payload: &[u8]) -> Result<Option<Vec<u8>>, UdsDispatchFailure> {
            let mut response = payload.to_vec();
            if let Some(first) = response.first_mut() {
                *first = first.wrapping_add(0x40);
            }
            Ok(Some(response))
        }
    }

    struct SuppressedHandler;

    impl UdsRequestHandler for SuppressedHandler {
        fn process_uds(&self, _payload: &[u8]) -> Result<Option<Vec<u8>>, UdsDispatchFailure> {
            Ok(None)
        }
    }

    fn active_connection() -> RoutingConnection {
        let mut connection = RoutingConnection::new(0x1101);
        let request = RoutingActivationRequest {
            tester_logical_address: 0x0E00,
            activation_type: 0x00,
            reserved: [0; 4],
        };
        connection.activate(&request).unwrap();
        connection
    }

    fn diagnostic_payload(source: u16, target: u16, uds: &[u8]) -> Vec<u8> {
        let mut payload = Vec::with_capacity(4 + uds.len());
        payload.extend_from_slice(&source.to_be_bytes());
        payload.extend_from_slice(&target.to_be_bytes());
        payload.extend_from_slice(uds);
        payload
    }

    #[test]
    fn routes_to_registered_ecu_and_reverses_addresses() {
        let mut router = DiagnosticRouter::new();
        router.register(0x1110, Arc::new(EchoHandler)).unwrap();

        let payload = diagnostic_payload(0x0E00, 0x1110, &[0x22, 0xF1, 0x90]);
        let response = router.route(&active_connection(), &payload).unwrap();

        assert_eq!(response.source_address, 0x1110);
        assert_eq!(response.target_address, 0x0E00);
        assert_eq!(response.uds_payload, Some(vec![0x62, 0xF1, 0x90]));
    }

    #[test]
    fn rejects_diagnostic_before_routing_activation() {
        let router = DiagnosticRouter::new();
        let connection = RoutingConnection::new(0x1101);
        let payload = diagnostic_payload(0x0E00, 0x1110, &[0x22]);

        assert_eq!(
            router.route(&connection, &payload),
            Err(RouterError::RoutingNotActive)
        );
    }

    #[test]
    fn rejects_mismatched_tester_address() {
        let mut router = DiagnosticRouter::new();
        router.register(0x1110, Arc::new(EchoHandler)).unwrap();

        let payload = diagnostic_payload(0x0E01, 0x1110, &[0x22]);

        assert_eq!(
            router.route(&active_connection(), &payload),
            Err(RouterError::TesterAddressMismatch)
        );
    }

    #[test]
    fn rejects_unknown_target_address() {
        let router = DiagnosticRouter::new();
        let payload = diagnostic_payload(0x0E00, 0x9999, &[0x22]);

        assert_eq!(
            router.route(&active_connection(), &payload),
            Err(RouterError::UnknownTargetAddress)
        );
    }

    #[test]
    fn rejects_duplicate_target_registration() {
        let mut router = DiagnosticRouter::new();

        router.register(0x1110, Arc::new(EchoHandler)).unwrap();

        assert_eq!(
            router.register(0x1110, Arc::new(EchoHandler)),
            Err(RouterError::DuplicateTargetAddress)
        );

        assert_eq!(router.target_count(), 1);
    }

    #[test]
    fn preserves_suppressed_uds_response() {
        let mut router = DiagnosticRouter::new();
        router
            .register(0x1110, Arc::new(SuppressedHandler))
            .unwrap();

        let payload = diagnostic_payload(0x0E00, 0x1110, &[0x3E, 0x80]);
        let response = router.route(&active_connection(), &payload).unwrap();

        assert_eq!(response.uds_payload, None);
    }

    #[test]
    fn rejects_empty_uds_payload() {
        let router = DiagnosticRouter::new();
        let payload = diagnostic_payload(0x0E00, 0x1110, &[]);

        assert!(matches!(
            router.route(&active_connection(), &payload),
            Err(RouterError::InvalidDiagnosticPayload(
                DiagnosticPayloadError::EmptyUdsPayload
            ))
        ));
    }
}
