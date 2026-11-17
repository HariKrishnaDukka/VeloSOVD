//! DoIP diagnostic message payload parsing and encoding.
//!
//! Wire format:
//! - Bytes 0..2: source logical address (big-endian).
//! - Bytes 2..4: target logical address (big-endian).
//! - Bytes 4..: non-empty UDS payload.

use std::error::Error;
use std::fmt;

/// Minimum diagnostic payload size: two addresses and one UDS byte.
pub const DIAGNOSTIC_MESSAGE_MIN_LENGTH: usize = 5;

/// A borrowed DoIP diagnostic message.
///
/// The UDS payload is borrowed from the received frame to avoid an
/// unnecessary allocation during parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticMessage<'a> {
    pub source_address: u16,
    pub target_address: u16,
    pub uds_payload: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticPayloadError {
    PayloadTooShort,
    EmptyUdsPayload,
}

impl fmt::Display for DiagnosticPayloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PayloadTooShort => {
                write!(
                    f,
                    "DoIP diagnostic payload is shorter than four address bytes"
                )
            }
            Self::EmptyUdsPayload => {
                write!(f, "DoIP diagnostic payload contains no UDS data")
            }
        }
    }
}

impl Error for DiagnosticPayloadError {}

impl<'a> DiagnosticMessage<'a> {
    /// Decode a diagnostic message from its DoIP payload bytes.
    pub fn decode(payload: &'a [u8]) -> Result<Self, DiagnosticPayloadError> {
        if payload.len() < 4 {
            return Err(DiagnosticPayloadError::PayloadTooShort);
        }

        if payload.len() == 4 {
            return Err(DiagnosticPayloadError::EmptyUdsPayload);
        }

        let source_address = u16::from_be_bytes([payload[0], payload[1]]);
        let target_address = u16::from_be_bytes([payload[2], payload[3]]);

        Ok(Self {
            source_address,
            target_address,
            uds_payload: &payload[4..],
        })
    }

    /// Encode this message into a DoIP diagnostic payload.
    pub fn encode(&self) -> Vec<u8> {
        let mut payload = Vec::with_capacity(4 + self.uds_payload.len());

        payload.extend_from_slice(&self.source_address.to_be_bytes());
        payload.extend_from_slice(&self.target_address.to_be_bytes());
        payload.extend_from_slice(self.uds_payload);

        payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_addresses_and_uds_payload() {
        let raw = [0x0E, 0x00, 0x11, 0x10, 0x22, 0xF1, 0x90];

        let message = DiagnosticMessage::decode(&raw).unwrap();

        assert_eq!(message.source_address, 0x0E00);
        assert_eq!(message.target_address, 0x1110);
        assert_eq!(message.uds_payload, &[0x22, 0xF1, 0x90]);
    }

    #[test]
    fn encodes_addresses_in_network_byte_order() {
        let message = DiagnosticMessage {
            source_address: 0x0E00,
            target_address: 0x1110,
            uds_payload: &[0x22, 0xF1, 0x90],
        };

        assert_eq!(
            message.encode(),
            vec![0x0E, 0x00, 0x11, 0x10, 0x22, 0xF1, 0x90]
        );
    }

    #[test]
    fn decode_and_encode_round_trip() {
        let raw = [0x0E, 0x00, 0x11, 0x10, 0x3E, 0x00];

        let decoded = DiagnosticMessage::decode(&raw).unwrap();

        assert_eq!(decoded.encode(), raw);
    }

    #[test]
    fn rejects_payload_shorter_than_address_fields() {
        assert_eq!(
            DiagnosticMessage::decode(&[0x0E, 0x00, 0x11]),
            Err(DiagnosticPayloadError::PayloadTooShort)
        );
    }

    #[test]
    fn rejects_empty_uds_payload() {
        assert_eq!(
            DiagnosticMessage::decode(&[0x0E, 0x00, 0x11, 0x10]),
            Err(DiagnosticPayloadError::EmptyUdsPayload)
        );
    }
}
