use std::collections::HashMap;

use crate::DidEntry;
use crate::response::negative_response;
use crate::session::DiagnosticSession;

/// Handle UDS ReadDataByIdentifier (SID 0x22).
///
/// This initial implementation supports one or more two-byte DIDs.
/// All DIDs are validated before constructing the response.
pub fn handle(
    payload: &[u8],
    current_session: DiagnosticSession,
    dids: &HashMap<u16, DidEntry>,
) -> Option<Vec<u8>> {
    if payload.len() < 3 || !(payload.len() - 1).is_multiple_of(2) {
        return Some(negative_response(0x22, 0x13));
    }

    let requested_dids: Vec<u16> = payload[1..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| u16::from_be_bytes(*bytes))
        .collect();

    // Validate every DID first. Do not return partial data.
    for identifier in &requested_dids {
        let Some(entry) = dids.get(identifier) else {
            return Some(negative_response(0x22, 0x31));
        };

        if !entry.allowed_sessions.contains(&current_session) {
            return Some(negative_response(0x22, 0x31));
        }
    }

    let mut response = Vec::new();
    response.push(0x62);

    for identifier in requested_dids {
        let entry = &dids[&identifier];
        response.extend_from_slice(&identifier.to_be_bytes());
        response.extend_from_slice(&entry.data);
    }

    Some(response)
}
