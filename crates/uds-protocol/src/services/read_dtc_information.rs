//! UDS ReadDTCInformation (0x19), demonstrator subset.
//!
//! Implements subfunction 0x02 (reportDTCByStatusMask) only.

use crate::{DtcStore, response};

pub fn handle(payload: &[u8], store: &DtcStore) -> Option<Vec<u8>> {
    if payload.len() != 3 {
        return Some(response::negative_response(0x19, 0x13));
    }

    let subfunction = payload[1] & 0x7F;
    if subfunction != 0x02 {
        return Some(response::negative_response(0x19, 0x12));
    }

    let requested_mask = payload[2];
    let entries = match store.lock() {
        Ok(guard) => guard,
        Err(_) => return Some(response::negative_response(0x19, 0x22)),
    };

    // The demonstrator currently models all eight status bits.
    let mut result = vec![0x59, 0x02, 0xFF];
    for entry in entries
        .iter()
        .filter(|entry| entry.status & requested_mask != 0)
    {
        let dtc = entry.dtc & 0x00FF_FFFF;
        result.push((dtc >> 16) as u8);
        result.push((dtc >> 8) as u8);
        result.push(dtc as u8);
        result.push(entry.status);
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DtcEntry, new_dtc_store};

    #[test]
    fn reports_matching_dtcs_and_status() {
        let store = new_dtc_store(vec![
            DtcEntry {
                dtc: 0x000128,
                code: "P0128".into(),
                description: "Coolant temperature".into(),
                status: 0x09,
            },
            DtcEntry {
                dtc: 0x009000,
                code: "B1000".into(),
                description: "Body configuration".into(),
                status: 0x00,
            },
        ]);
        assert_eq!(
            handle(&[0x19, 0x02, 0x08], &store),
            Some(vec![0x59, 0x02, 0xFF, 0x00, 0x01, 0x28, 0x09])
        );
    }

    #[test]
    fn rejects_unsupported_subfunction() {
        let store = new_dtc_store(Vec::new());
        assert_eq!(
            handle(&[0x19, 0x01, 0xFF], &store),
            Some(vec![0x7F, 0x19, 0x12])
        );
    }

    #[test]
    fn rejects_bad_length() {
        let store = new_dtc_store(Vec::new());
        assert_eq!(handle(&[0x19, 0x02], &store), Some(vec![0x7F, 0x19, 0x13]));
    }
}
