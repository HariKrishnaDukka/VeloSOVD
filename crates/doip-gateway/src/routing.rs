// DoIP routing activation payload structures.

use std::fmt;

pub const ROUTING_ACTIVATION_REQUEST_LENGTH: usize = 7;
pub const ROUTING_ACTIVATION_RESPONSE_LENGTH: usize = 9;
pub const ROUTING_RESERVED_LENGTH: usize = 4;

/// Default diagnostic routing activation type.
pub const DEFAULT_DIAGNOSTIC_ACTIVATION_TYPE: u8 = 0x00;

/// Successful routing activation response code.
pub const ROUTING_RESPONSE_SUCCESS: u8 = 0x10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingPayloadError {
    InvalidLength,
}

impl fmt::Display for RoutingPayloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength => {
                write!(f, "Invalid routing activation payload length")
            }
        }
    }
}

impl std::error::Error for RoutingPayloadError {}

/// Routing Activation Request (payload type 0x0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutingActivationRequest {
    pub tester_logical_address: u16,
    pub activation_type: u8,
    pub reserved: [u8; ROUTING_RESERVED_LENGTH],
}

impl RoutingActivationRequest {
    pub fn decode(payload: &[u8]) -> Result<Self, RoutingPayloadError> {
        if payload.len() != ROUTING_ACTIVATION_REQUEST_LENGTH {
            return Err(RoutingPayloadError::InvalidLength);
        }

        let tester_logical_address = u16::from_be_bytes([payload[0], payload[1]]);

        let mut reserved = [0_u8; ROUTING_RESERVED_LENGTH];
        reserved.copy_from_slice(&payload[3..7]);

        Ok(Self {
            tester_logical_address,
            activation_type: payload[2],
            reserved,
        })
    }

    pub fn encode(&self) -> [u8; ROUTING_ACTIVATION_REQUEST_LENGTH] {
        let mut payload = [0_u8; ROUTING_ACTIVATION_REQUEST_LENGTH];

        payload[0..2].copy_from_slice(&self.tester_logical_address.to_be_bytes());
        payload[2] = self.activation_type;
        payload[3..7].copy_from_slice(&self.reserved);

        payload
    }
}

/// Routing Activation Response (payload type 0x0006).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutingActivationResponse {
    pub tester_logical_address: u16,
    pub entity_logical_address: u16,
    pub response_code: u8,
    pub reserved: [u8; ROUTING_RESERVED_LENGTH],
}

impl RoutingActivationResponse {
    pub fn decode(payload: &[u8]) -> Result<Self, RoutingPayloadError> {
        if payload.len() != ROUTING_ACTIVATION_RESPONSE_LENGTH {
            return Err(RoutingPayloadError::InvalidLength);
        }

        let tester_logical_address = u16::from_be_bytes([payload[0], payload[1]]);

        let entity_logical_address = u16::from_be_bytes([payload[2], payload[3]]);

        let mut reserved = [0_u8; ROUTING_RESERVED_LENGTH];
        reserved.copy_from_slice(&payload[5..9]);

        Ok(Self {
            tester_logical_address,
            entity_logical_address,
            response_code: payload[4],
            reserved,
        })
    }

    pub fn encode(&self) -> [u8; ROUTING_ACTIVATION_RESPONSE_LENGTH] {
        let mut payload = [0_u8; ROUTING_ACTIVATION_RESPONSE_LENGTH];

        payload[0..2].copy_from_slice(&self.tester_logical_address.to_be_bytes());
        payload[2..4].copy_from_slice(&self.entity_logical_address.to_be_bytes());
        payload[4] = self.response_code;
        payload[5..9].copy_from_slice(&self.reserved);

        payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_request_round_trip() {
        let request = RoutingActivationRequest {
            tester_logical_address: 0x0E00,
            activation_type: DEFAULT_DIAGNOSTIC_ACTIVATION_TYPE,
            reserved: [0; 4],
        };

        let decoded = RoutingActivationRequest::decode(&request.encode()).unwrap();

        assert_eq!(decoded, request);
    }

    #[test]
    fn rejects_invalid_request_length() {
        assert_eq!(
            RoutingActivationRequest::decode(&[0; 6]),
            Err(RoutingPayloadError::InvalidLength)
        );
    }

    #[test]
    fn routing_response_round_trip() {
        let response = RoutingActivationResponse {
            tester_logical_address: 0x0E00,
            entity_logical_address: 0x1101,
            response_code: ROUTING_RESPONSE_SUCCESS,
            reserved: [0; 4],
        };

        let decoded = RoutingActivationResponse::decode(&response.encode()).unwrap();

        assert_eq!(decoded, response);
    }

    #[test]
    fn rejects_invalid_response_length() {
        assert_eq!(
            RoutingActivationResponse::decode(&[0; 8]),
            Err(RoutingPayloadError::InvalidLength)
        );
    }

    #[test]
    fn encodes_logical_addresses_in_network_byte_order() {
        let request = RoutingActivationRequest {
            tester_logical_address: 0x0E00,
            activation_type: 0x00,
            reserved: [0; 4],
        };

        assert_eq!(request.encode(), [0x0E, 0x00, 0x00, 0, 0, 0, 0]);
    }
}
