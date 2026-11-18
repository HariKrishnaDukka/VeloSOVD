//! UDS ClearDiagnosticInformation (0x14), demonstrator subset.
//!
//! Only the all-DTC group (0xFFFFFF) is supported.

use crate::{DtcStore, response};

pub fn handle(payload: &[u8], store: &DtcStore) -> Option<Vec<u8>> {
    if payload.len() != 4 {
        return Some(response::negative_response(0x14, 0x13));
    }

    let group = ((payload[1] as u32) << 16) | ((payload[2] as u32) << 8) | payload[3] as u32;

    if group != 0x00FF_FFFF {
        return Some(response::negative_response(0x14, 0x31));
    }

    match store.lock() {
        Ok(mut entries) => entries.clear(),
        Err(_) => return Some(response::negative_response(0x14, 0x22)),
    }

    Some(vec![0x54])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DtcEntry, new_dtc_store};

    #[test]
    fn clears_shared_dtc_store() {
        let store = new_dtc_store(vec![DtcEntry {
            dtc: 0x000128,
            code: "P0128".into(),
            description: "Coolant temperature".into(),
            status: 0x09,
        }]);
        assert_eq!(handle(&[0x14, 0xFF, 0xFF, 0xFF], &store), Some(vec![0x54]));
        assert!(store.lock().unwrap().is_empty());
    }

    #[test]
    fn rejects_unsupported_group() {
        let store = new_dtc_store(Vec::new());
        assert_eq!(
            handle(&[0x14, 0x00, 0x00, 0x01], &store),
            Some(vec![0x7F, 0x14, 0x31])
        );
    }

    #[test]
    fn clearing_empty_store_returns_positive_response() {
        let store = new_dtc_store(Vec::new());

        assert_eq!(handle(&[0x14, 0xFF, 0xFF, 0xFF], &store), Some(vec![0x54]));
        assert!(store.lock().unwrap().is_empty());
    }

    #[test]
    fn rejects_invalid_request_length_with_nrc_13() {
        let store = new_dtc_store(Vec::new());

        assert_eq!(
            handle(&[0x14, 0xFF, 0xFF], &store),
            Some(vec![0x7F, 0x14, 0x13])
        );
    }
}
