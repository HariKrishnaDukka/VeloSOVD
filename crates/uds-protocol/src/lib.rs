//! Transport-independent UDS protocol engine.
//!
//! This crate processes complete UDS application payloads.
//! CAN, ISO-TP, DoIP and HTTP are intentionally outside this crate.

pub mod request;
pub mod response;
pub mod services;
pub mod session;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use services::{
    clear_diagnostic_information, diagnostic_session, read_data_by_identifier,
    read_dtc_information, tester_present,
};
use session::{DiagnosticSession, SessionManager};

pub use session::DEFAULT_S3_TIMEOUT;
pub use session::DiagnosticSession as UdsSession;

/// A configured Data Identifier.
#[derive(Debug, Clone)]
pub struct DidEntry {
    pub identifier: u16,
    pub data: Vec<u8>,
    pub allowed_sessions: Vec<DiagnosticSession>,
}

/// One diagnostic trouble code and its UDS status byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DtcEntry {
    /// Three-byte ISO 14229 DTC value, stored in the low 24 bits.
    pub dtc: u32,
    /// Human-readable code used by the virtual ECU HTTP API (for example P0128).
    pub code: String,
    pub description: String,
    /// ISO 14229 DTC status byte.
    pub status: u8,
}

/// Shared per-ECU DTC memory. HTTP and UDS interfaces must use the same instance.
pub type DtcStore = Arc<Mutex<Vec<DtcEntry>>>;

pub fn new_dtc_store(entries: Vec<DtcEntry>) -> DtcStore {
    Arc::new(Mutex::new(entries))
}

/// UDS diagnostic server state for one ECU.
#[derive(Debug)]
pub struct UdsServer {
    session: SessionManager,
    dids: HashMap<u16, DidEntry>,
    dtcs: DtcStore,
}

impl Default for UdsServer {
    fn default() -> Self {
        Self::with_dtc_store(new_dtc_store(Vec::new()))
    }
}

impl UdsServer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_dtc_store(dtcs: DtcStore) -> Self {
        Self {
            session: SessionManager::default(),
            dids: HashMap::new(),
            dtcs,
        }
    }

    pub fn with_s3_timeout(timeout: Duration) -> Self {
        Self::with_s3_timeout_and_dtc_store(timeout, new_dtc_store(Vec::new()))
    }

    pub fn with_s3_timeout_and_dtc_store(timeout: Duration, dtcs: DtcStore) -> Self {
        Self {
            session: SessionManager::with_s3_timeout(timeout),
            dids: HashMap::new(),
            dtcs,
        }
    }

    pub fn dtc_store(&self) -> DtcStore {
        Arc::clone(&self.dtcs)
    }

    /// Register or replace a DID.
    pub fn register_did(&mut self, entry: DidEntry) {
        self.dids.insert(entry.identifier, entry);
    }

    pub fn current_session(&self) -> DiagnosticSession {
        self.session.current()
    }

    /// Process one complete UDS request.
    ///
    /// `None` means a positive response was intentionally suppressed.
    /// Negative responses are never suppressed.
    pub fn process_request(&mut self, payload: &[u8]) -> Option<Vec<u8>> {
        self.process_request_at(payload, Instant::now())
    }

    pub fn process_request_at(&mut self, payload: &[u8], now: Instant) -> Option<Vec<u8>> {
        self.session.expire_if_needed(now);

        let Some(&sid) = payload.first() else {
            return Some(response::negative_response(0x00, 0x13));
        };

        match sid {
            0x10 => diagnostic_session::handle(payload, &mut self.session, now),
            0x22 => {
                let result =
                    read_data_by_identifier::handle(payload, self.session.current(), &self.dids);
                if result.as_ref().is_some_and(|r| r.first() == Some(&0x62)) {
                    self.session.refresh_activity_at(now);
                }
                result
            }
            0x19 => {
                let result = read_dtc_information::handle(payload, &self.dtcs);
                if result.as_ref().is_some_and(|r| r.first() == Some(&0x59)) {
                    self.session.refresh_activity_at(now);
                }
                result
            }
            0x14 => {
                let result = clear_diagnostic_information::handle(payload, &self.dtcs);
                if result.as_ref().is_some_and(|r| r.first() == Some(&0x54)) {
                    self.session.refresh_activity_at(now);
                }
                result
            }
            0x3E => {
                let result = tester_present::handle(payload);
                if result.as_ref().is_none_or(|r| r.first() == Some(&0x7E)) {
                    self.session.refresh_activity_at(now);
                }
                result
            }
            _ => Some(response::negative_response(sid, 0x11)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_server() -> UdsServer {
        let mut server = UdsServer::new();

        server.register_did(DidEntry {
            identifier: 0xF190,
            data: b"SDVDEMO0000000001".to_vec(),
            allowed_sessions: vec![DiagnosticSession::Default, DiagnosticSession::Extended],
        });

        server.register_did(DidEntry {
            identifier: 0xF187,
            data: b"ECM-SOFTWARE-001".to_vec(),
            allowed_sessions: vec![DiagnosticSession::Extended],
        });

        server
    }

    #[test]
    fn default_session_is_active_at_startup() {
        let server = test_server();
        assert_eq!(server.current_session(), DiagnosticSession::Default);
    }

    #[test]
    fn enters_extended_session() {
        let mut server = test_server();

        let response = server.process_request(&[0x10, 0x03]);

        assert_eq!(response, Some(vec![0x50, 0x03, 0x00, 0x32, 0x01, 0xF4]));
        assert_eq!(server.current_session(), DiagnosticSession::Extended);
    }

    #[test]
    fn rejects_unsupported_session() {
        let mut server = test_server();

        let response = server.process_request(&[0x10, 0x02]);

        assert_eq!(response, Some(vec![0x7F, 0x10, 0x12]));
    }

    #[test]
    fn reads_vin_in_default_session() {
        let mut server = test_server();

        let response = server.process_request(&[0x22, 0xF1, 0x90]).unwrap();

        assert_eq!(&response[..3], &[0x62, 0xF1, 0x90]);
        assert_eq!(&response[3..], b"SDVDEMO0000000001");
    }

    #[test]
    fn rejects_extended_only_did_in_default_session() {
        let mut server = test_server();

        assert_eq!(
            server.process_request(&[0x22, 0xF1, 0x87]),
            Some(vec![0x7F, 0x22, 0x31])
        );
    }

    #[test]
    fn reads_extended_only_did_after_session_change() {
        let mut server = test_server();

        server.process_request(&[0x10, 0x03]);

        let response = server.process_request(&[0x22, 0xF1, 0x87]).unwrap();

        assert_eq!(&response[..3], &[0x62, 0xF1, 0x87]);
        assert_eq!(&response[3..], b"ECM-SOFTWARE-001");
    }

    #[test]
    fn rejects_invalid_did_request_length() {
        let mut server = test_server();

        assert_eq!(
            server.process_request(&[0x22, 0xF1]),
            Some(vec![0x7F, 0x22, 0x13])
        );
    }

    #[test]
    fn rejects_unknown_did() {
        let mut server = test_server();

        assert_eq!(
            server.process_request(&[0x22, 0x12, 0x34]),
            Some(vec![0x7F, 0x22, 0x31])
        );
    }

    #[test]
    fn suppresses_positive_session_response() {
        let mut server = test_server();

        let response = server.process_request(&[0x10, 0x83]);

        assert_eq!(response, None);
        assert_eq!(server.current_session(), DiagnosticSession::Extended);
    }

    #[test]
    fn s3_timeout_returns_to_default_session() {
        let start = Instant::now();
        let mut server = UdsServer::with_s3_timeout(Duration::from_secs(5));
        server.process_request_at(&[0x10, 0x03], start);
        server.process_request_at(&[0x3E, 0x00], start + Duration::from_secs(4));
        server.process_request_at(&[0x3E, 0x00], start + Duration::from_secs(10));
        assert_eq!(server.current_session(), DiagnosticSession::Default);
    }

    #[test]
    fn rejects_unknown_service() {
        let mut server = test_server();

        assert_eq!(
            server.process_request(&[0x99]),
            Some(vec![0x7F, 0x99, 0x11])
        );
    }
}
