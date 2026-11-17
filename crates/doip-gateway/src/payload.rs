//! DoIP payload type definitions.
//!
//! This module identifies payload types from the DoIP generic header.
//! Payload-specific parsing is implemented in separate modules.

/// DoIP payload types defined by the protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum DoipPayloadType {
    GenericHeaderNack = 0x0000,

    VehicleIdentificationRequest = 0x0001,
    VehicleIdentificationRequestWithEid = 0x0002,
    VehicleIdentificationRequestWithVin = 0x0003,
    VehicleAnnouncementOrIdentificationResponse = 0x0004,

    RoutingActivationRequest = 0x0005,
    RoutingActivationResponse = 0x0006,

    AliveCheckRequest = 0x0007,
    AliveCheckResponse = 0x0008,

    DiagnosticMessage = 0x8001,
    DiagnosticMessagePositiveAck = 0x8002,
    DiagnosticMessageNegativeAck = 0x8003,

    EntityStatusRequest = 0x4001,
    EntityStatusResponse = 0x4002,

    DiagnosticPowerModeRequest = 0x4003,
    DiagnosticPowerModeResponse = 0x4004,
}

impl DoipPayloadType {
    /// Converts a raw 16-bit payload type into a known DoIP payload type.
    pub fn from_raw(value: u16) -> Option<Self> {
        match value {
            0x0000 => Some(Self::GenericHeaderNack),
            0x0001 => Some(Self::VehicleIdentificationRequest),
            0x0002 => Some(Self::VehicleIdentificationRequestWithEid),
            0x0003 => Some(Self::VehicleIdentificationRequestWithVin),
            0x0004 => Some(Self::VehicleAnnouncementOrIdentificationResponse),
            0x0005 => Some(Self::RoutingActivationRequest),
            0x0006 => Some(Self::RoutingActivationResponse),
            0x0007 => Some(Self::AliveCheckRequest),
            0x0008 => Some(Self::AliveCheckResponse),
            0x8001 => Some(Self::DiagnosticMessage),
            0x8002 => Some(Self::DiagnosticMessagePositiveAck),
            0x8003 => Some(Self::DiagnosticMessageNegativeAck),
            0x4001 => Some(Self::EntityStatusRequest),
            0x4002 => Some(Self::EntityStatusResponse),
            0x4003 => Some(Self::DiagnosticPowerModeRequest),
            0x4004 => Some(Self::DiagnosticPowerModeResponse),
            _ => None,
        }
    }

    /// Returns the raw 16-bit payload type value.
    pub const fn as_raw(self) -> u16 {
        self as u16
    }
}

#[cfg(test)]
mod tests {
    use super::DoipPayloadType;

    #[test]
    fn decodes_diagnostic_message_type() {
        assert_eq!(
            DoipPayloadType::from_raw(0x8001),
            Some(DoipPayloadType::DiagnosticMessage)
        );
    }

    #[test]
    fn decodes_routing_activation_request() {
        assert_eq!(
            DoipPayloadType::from_raw(0x0005),
            Some(DoipPayloadType::RoutingActivationRequest)
        );
    }

    #[test]
    fn rejects_unknown_payload_type() {
        assert_eq!(DoipPayloadType::from_raw(0x1234), None);
    }

    #[test]
    fn converts_payload_type_back_to_raw_value() {
        assert_eq!(DoipPayloadType::DiagnosticMessage.as_raw(), 0x8001);
    }
}
