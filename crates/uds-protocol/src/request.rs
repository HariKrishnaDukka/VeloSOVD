//! UDS request constants and basic request helpers.

pub const SID_DIAGNOSTIC_SESSION_CONTROL: u8 = 0x10;
pub const SID_READ_DATA_BY_IDENTIFIER: u8 = 0x22;
pub const SID_TESTER_PRESENT: u8 = 0x3E;
pub const SID_NEGATIVE_RESPONSE: u8 = 0x7F;

pub fn service_id(payload: &[u8]) -> Option<u8> {
    payload.first().copied()
}
