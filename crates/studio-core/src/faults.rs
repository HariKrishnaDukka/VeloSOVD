//! M3 step 1: fault explorer rules. No network, no GUI - plain functions that the
//! Tauri shell calls before it sends anything, so they are unit-tested in CI.
//!
//! In C terms: a header of small pure helpers - a bitmask decoder, a permission
//! check, and a function that builds (and refuses to build) a request string.

use serde::Serialize;

use crate::session::{StudioError, validate_path};

/// One bit of the ISO 14229 DTC status byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StatusBit {
    pub bit: u8,
    pub name: &'static str,
    pub set: bool,
}

/// Names of the 8 DTC status bits, bit 0 first (ISO 14229-1 DTC status byte).
pub const STATUS_BIT_NAMES: [&str; 8] = [
    "testFailed",
    "testFailedThisOperationCycle",
    "pendingDTC",
    "confirmedDTC",
    "testNotCompletedSinceLastClear",
    "testFailedSinceLastClear",
    "testNotCompletedThisOperationCycle",
    "warningIndicatorRequested",
];

/// Split the raw status byte into its 8 named bits (like `status & (1 << n)` in C).
pub fn decode_status(status: u8) -> [StatusBit; 8] {
    std::array::from_fn(|i| StatusBit {
        bit: i as u8,
        name: STATUS_BIT_NAMES[i],
        set: status & (1u8 << i) != 0,
    })
}

/// "Active" as central reports it: bit 0 (testFailed).
pub fn is_active(status: u8) -> bool {
    status & 0x01 != 0
}

/// Roles in increasing power. The derive gives `Viewer < Technician < Engineer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Viewer,
    Technician,
    Engineer,
}

impl Role {
    /// Exact lowercase names as they appear in the login token. Anything else is unknown.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "viewer" => Some(Self::Viewer),
            "technician" => Some(Self::Technician),
            "engineer" => Some(Self::Engineer),
            _ => None,
        }
    }
}

/// May this role clear faults or stop operations? Technician and up. Unknown roles: no.
/// This only decides what Studio offers; central enforces the same rule (403).
pub fn may_change(role: &str) -> bool {
    Role::parse(role).is_some_and(|r| r >= Role::Technician)
}

/// ECU ids and fault codes become URL path parts: letters, digits, `_` and `-` only.
fn check_segment(kind: &str, value: &str) -> Result<(), StudioError> {
    let ok = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(StudioError::InvalidRequest(format!("invalid {kind}")))
    }
}

/// `GET` path of the fault list, optionally filtered by active state.
pub fn fault_list_path(ecu: &str, active: Option<bool>) -> Result<String, StudioError> {
    check_segment("ECU id", ecu)?;
    let mut path = format!("/api/ecus/{ecu}/diagnostics/faults");
    match active {
        Some(true) => path.push_str("?active=true"),
        Some(false) => path.push_str("?active=false"),
        None => {}
    }
    validate_path(&path)?;
    Ok(path)
}

/// `GET` / `DELETE` path of one fault.
pub fn fault_path(ecu: &str, code: &str) -> Result<String, StudioError> {
    check_segment("ECU id", ecu)?;
    check_segment("fault code", code)?;
    let path = format!("/api/ecus/{ecu}/diagnostics/faults/{code}");
    validate_path(&path)?;
    Ok(path)
}

/// A clear request that passed every check and may now be sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClearPlan {
    pub method: &'static str,
    pub path: String,
}

/// Decide whether a clear may be sent. Order: role first, then confirmation, then path.
/// `confirmed_ecu` is the ECU id the user typed or confirmed in the dialog; it must equal
/// `ecu` exactly. `code = None` clears all faults of the ECU.
pub fn plan_clear(
    role: &str,
    ecu: &str,
    code: Option<&str>,
    confirmed_ecu: Option<&str>,
) -> Result<ClearPlan, StudioError> {
    if !may_change(role) {
        return Err(StudioError::InvalidRequest(
            "your role may not clear faults".into(),
        ));
    }
    if confirmed_ecu != Some(ecu) {
        return Err(StudioError::InvalidRequest(
            "clearing needs confirmation: repeat the ECU id".into(),
        ));
    }
    let path = match code {
        Some(code) => fault_path(ecu, code)?,
        None => fault_list_path(ecu, None)?,
    };
    Ok(ClearPlan {
        method: "DELETE",
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_each_status_bit() {
        for i in 0..8u8 {
            let bits = decode_status(1u8 << i);
            for b in bits.iter() {
                assert_eq!(b.set, b.bit == i, "status bit {i}, checking bit {}", b.bit);
            }
            assert_eq!(bits[i as usize].name, STATUS_BIT_NAMES[i as usize]);
        }
        assert!(decode_status(0x00).iter().all(|b| !b.set));
        assert!(decode_status(0xFF).iter().all(|b| b.set));
        // example: 0x09 = testFailed + confirmedDTC
        let example = decode_status(0x09);
        let set: Vec<&str> = example.iter().filter(|b| b.set).map(|b| b.name).collect();
        assert_eq!(set, ["testFailed", "confirmedDTC"]);
    }

    #[test]
    fn active_follows_bit0() {
        for status in 0..=255u8 {
            assert_eq!(is_active(status), decode_status(status)[0].set);
            assert_eq!(is_active(status), status % 2 == 1);
        }
    }

    #[test]
    fn clear_actions_need_technician() {
        assert!(!may_change("viewer"));
        assert!(may_change("technician"));
        assert!(may_change("engineer"));
        for bad in ["", "admin", "Technician", "ENGINEER", "viewer "] {
            assert!(!may_change(bad), "{bad:?}");
        }
        // a viewer cannot plan a clear, whatever else is right
        let denied = plan_clear("viewer", "ECM", None, Some("ECM")).unwrap_err();
        assert!(matches!(denied, StudioError::InvalidRequest(_)));
        assert!(plan_clear("technician", "ECM", None, Some("ECM")).is_ok());
    }

    #[test]
    fn clear_without_confirmation_is_refused() {
        for confirm in [None, Some(""), Some("ecm"), Some("BCM"), Some("ECM ")] {
            let all = plan_clear("engineer", "ECM", None, confirm);
            assert!(matches!(all, Err(StudioError::InvalidRequest(_))), "{confirm:?}");
            assert!(plan_clear("engineer", "ECM", Some("P0101"), confirm).is_err());
        }
        let all = plan_clear("technician", "ECM", None, Some("ECM")).unwrap();
        assert_eq!(
            all,
            ClearPlan {
                method: "DELETE",
                path: "/api/ecus/ECM/diagnostics/faults".into()
            }
        );
        let one = plan_clear("technician", "ECM", Some("P0101"), Some("ECM")).unwrap();
        assert_eq!(one.path, "/api/ecus/ECM/diagnostics/faults/P0101");
    }

    #[test]
    fn explorer_paths_stay_under_api_ecus() {
        assert_eq!(
            fault_list_path("ECM", None).unwrap(),
            "/api/ecus/ECM/diagnostics/faults"
        );
        assert_eq!(
            fault_list_path("ECM", Some(true)).unwrap(),
            "/api/ecus/ECM/diagnostics/faults?active=true"
        );
        assert_eq!(
            fault_list_path("HVAC", Some(false)).unwrap(),
            "/api/ecus/HVAC/diagnostics/faults?active=false"
        );
        assert_eq!(
            fault_path("ECM", "P0101").unwrap(),
            "/api/ecus/ECM/diagnostics/faults/P0101"
        );
        let long = "A".repeat(65);
        for bad in [
            "", "..", "a/b", "a?b", "a#b", "a b", "a\\b", "ECM/../x", "\u{e9}", "a\nb",
            long.as_str(),
        ] {
            assert!(fault_list_path(bad, None).is_err(), "{bad:?}");
            assert!(fault_path("ECM", bad).is_err(), "{bad:?}");
            assert!(fault_path(bad, "P0101").is_err(), "{bad:?}");
        }
        for ok in [
            fault_list_path("LIGHTING", Some(true)).unwrap(),
            fault_path("ECM", "P0128").unwrap(),
        ] {
            assert!(validate_path(&ok).is_ok(), "{ok}");
        }
    }
}
