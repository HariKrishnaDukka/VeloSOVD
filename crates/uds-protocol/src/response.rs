//! UDS response construction.

pub fn negative_response(request_sid: u8, nrc: u8) -> Vec<u8> {
    vec![0x7F, request_sid, nrc]
}
