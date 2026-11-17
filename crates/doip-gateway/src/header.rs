//! DoIP generic header encoding and decoding.
//!
//! The generic header is eight bytes:
//! protocol version, inverse version, payload type, payload length.

use std::fmt;

pub const HEADER_LENGTH: usize = 8;
pub const DOIP_VERSION: u8 = 0x02;
pub const DEFAULT_MAX_PAYLOAD_LENGTH: u32 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DoipHeader {
    pub protocol_version: u8,
    pub payload_type: u16,
    pub payload_length: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderError {
    InvalidHeaderLength,
    InvalidVersion,
    PayloadTooLarge,
}

impl fmt::Display for HeaderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHeaderLength => write!(formatter, "Invalid DoIP header length"),
            Self::InvalidVersion => write!(formatter, "Invalid DoIP protocol version"),
            Self::PayloadTooLarge => write!(formatter, "DoIP payload exceeds configured limit"),
        }
    }
}

impl std::error::Error for HeaderError {}

impl DoipHeader {
    pub fn new(payload_type: u16, payload_length: u32) -> Self {
        Self {
            protocol_version: DOIP_VERSION,
            payload_type,
            payload_length,
        }
    }

    pub fn encode(&self) -> [u8; HEADER_LENGTH] {
        let mut bytes = [0_u8; HEADER_LENGTH];

        bytes[0] = self.protocol_version;
        bytes[1] = !self.protocol_version;
        bytes[2..4].copy_from_slice(&self.payload_type.to_be_bytes());
        bytes[4..8].copy_from_slice(&self.payload_length.to_be_bytes());

        bytes
    }

    pub fn decode(bytes: &[u8], max_payload_length: u32) -> Result<Self, HeaderError> {
        if bytes.len() != HEADER_LENGTH {
            return Err(HeaderError::InvalidHeaderLength);
        }

        let protocol_version = bytes[0];

        if bytes[1] != !protocol_version || protocol_version != DOIP_VERSION {
            return Err(HeaderError::InvalidVersion);
        }

        let payload_type = u16::from_be_bytes([bytes[2], bytes[3]]);
        let payload_length = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);

        if payload_length > max_payload_length {
            return Err(HeaderError::PayloadTooLarge);
        }

        Ok(Self {
            protocol_version,
            payload_type,
            payload_length,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_header_in_network_byte_order() {
        let header = DoipHeader::new(0x8001, 0x0000_0123);

        assert_eq!(
            header.encode(),
            [0x02, 0xFD, 0x80, 0x01, 0x00, 0x00, 0x01, 0x23]
        );
    }

    #[test]
    fn decodes_valid_header() {
        let bytes = [0x02, 0xFD, 0x80, 0x01, 0x00, 0x00, 0x00, 0x03];

        let header = DoipHeader::decode(&bytes, 4096).unwrap();

        assert_eq!(header.payload_type, 0x8001);
        assert_eq!(header.payload_length, 3);
        assert_eq!(header.protocol_version, DOIP_VERSION);
    }

    #[test]
    fn rejects_invalid_inverse_version() {
        let bytes = [0x02, 0x00, 0x00, 0x01, 0, 0, 0, 0];

        assert_eq!(
            DoipHeader::decode(&bytes, 4096),
            Err(HeaderError::InvalidVersion)
        );
    }

    #[test]
    fn rejects_unsupported_protocol_version() {
        let bytes = [0x03, 0xFC, 0x00, 0x01, 0, 0, 0, 0];

        assert_eq!(
            DoipHeader::decode(&bytes, 4096),
            Err(HeaderError::InvalidVersion)
        );
    }

    #[test]
    fn rejects_oversized_payload() {
        let bytes = [0x02, 0xFD, 0x00, 0x01, 0, 0, 0x20, 0x00];

        assert_eq!(
            DoipHeader::decode(&bytes, 4096),
            Err(HeaderError::PayloadTooLarge)
        );
    }

    #[test]
    fn rejects_truncated_header() {
        let bytes = [0x02, 0xFD, 0x00, 0x01];

        assert_eq!(
            DoipHeader::decode(&bytes, 4096),
            Err(HeaderError::InvalidHeaderLength)
        );
    }
}
