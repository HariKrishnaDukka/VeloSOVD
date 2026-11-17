use crate::response::negative_response;
use crate::session::{DiagnosticSession, SessionManager};
use std::time::Instant;

/// Handle UDS DiagnosticSessionControl (SID 0x10).
pub fn handle(payload: &[u8], session: &mut SessionManager, now: Instant) -> Option<Vec<u8>> {
    if payload.len() != 2 {
        return Some(negative_response(0x10, 0x13));
    }

    let suppress_positive_response = (payload[1] & 0x80) != 0;
    let requested_session = payload[1] & 0x7F;

    let target = match requested_session {
        0x01 => DiagnosticSession::Default,
        0x03 => DiagnosticSession::Extended,
        _ => return Some(negative_response(0x10, 0x12)),
    };

    session.transition_at(target, now);

    if suppress_positive_response {
        return None;
    }

    Some(vec![0x50, requested_session, 0x00, 0x32, 0x01, 0xF4])
}
