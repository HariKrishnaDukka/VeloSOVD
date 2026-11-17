//! UDS TesterPresent service (SID 0x3E).

use crate::response::negative_response;

pub fn handle(payload: &[u8]) -> Option<Vec<u8>> {
    if payload.len() != 2 {
        return Some(negative_response(0x3E, 0x13));
    }

    let suppress = (payload[1] & 0x80) != 0;
    if payload[1] & 0x7F != 0x00 {
        return Some(negative_response(0x3E, 0x12));
    }

    if suppress {
        None
    } else {
        Some(vec![0x7E, 0x00])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tester_present_positive_response() {
        assert_eq!(handle(&[0x3E, 0x00]), Some(vec![0x7E, 0x00]));
    }

    #[test]
    fn tester_present_suppressed_response() {
        assert_eq!(handle(&[0x3E, 0x80]), None);
    }

    #[test]
    fn tester_present_rejects_bad_subfunction() {
        assert_eq!(handle(&[0x3E, 0x01]), Some(vec![0x7F, 0x3E, 0x12]));
    }
}
