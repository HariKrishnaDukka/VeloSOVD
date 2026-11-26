//! Raw UDS request validation and the R1 service allow-list.

use axum::http::StatusCode;

pub(crate) fn validate_payload_hex(input: &str) -> Result<String, StatusCode> {
    if input.len() > 8192 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    let compact: String = input
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect();

    if compact.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    if compact.len() > 8192 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    let (byte_pairs, remainder) = compact.as_bytes().as_chunks::<2>();

    if !remainder.is_empty()
        || !byte_pairs
            .iter()
            .all(|pair| pair.iter().all(u8::is_ascii_hexdigit))
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    Ok(compact)
}

/// UDS services the raw passthrough endpoint may forward.
///
/// Deny by default: only read-only diagnostics and session keep-alive are
/// allowed. Write (0x2E), reset (0x11), routines (0x31), security access
/// (0x27), authentication (0x29), communication control (0x28),
/// DTC setting (0x85), clear DTC (0x14) and all download / upload / transfer
/// services (0x34-0x38) are refused here; they must go through dedicated,
/// authorized SOVD resources.
pub(crate) const RAW_UDS_ALLOWED_SIDS: [u8; 4] = [0x10, 0x19, 0x22, 0x3E];

/// DiagnosticSessionControl sub-functions allowed through the raw endpoint.
/// Programming session (0x02) and vendor sessions are refused.
pub(crate) const RAW_UDS_ALLOWED_SESSIONS: [u8; 2] = [0x01, 0x03];

/// Apply the raw UDS passthrough policy to a validated, normalized hex payload.
pub(crate) fn check_raw_uds_policy(normalized_hex: &str) -> Result<(), StatusCode> {
    let sid = u8::from_str_radix(normalized_hex.get(0..2).ok_or(StatusCode::BAD_REQUEST)?, 16)
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    if !RAW_UDS_ALLOWED_SIDS.contains(&sid) {
        return Err(StatusCode::FORBIDDEN);
    }

    if sid == 0x10 {
        let sub = u8::from_str_radix(normalized_hex.get(2..4).ok_or(StatusCode::BAD_REQUEST)?, 16)
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        // Bit 7 is suppressPosRspMsgIndicationBit; policy applies to the session type.
        if !RAW_UDS_ALLOWED_SESSIONS.contains(&(sub & 0x7F)) {
            return Err(StatusCode::FORBIDDEN);
        }
    }

    Ok(())
}

#[cfg(test)]
mod payload_validation_tests {
    use super::validate_payload_hex;
    use axum::http::StatusCode;

    #[test]
    fn accepts_normal_hex_payload() {
        assert_eq!(validate_payload_hex("22F190"), Ok("22F190".to_string()));
    }

    #[test]
    fn removes_ascii_whitespace() {
        assert_eq!(validate_payload_hex("22 F1 90"), Ok("22F190".to_string()));
    }

    #[test]
    fn accepts_mixed_case_hex() {
        assert_eq!(validate_payload_hex("22 f1 F0"), Ok("22f1F0".to_string()));
    }

    #[test]
    fn rejects_empty_payload() {
        assert_eq!(validate_payload_hex(""), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn rejects_whitespace_only_payload() {
        assert_eq!(validate_payload_hex("   "), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn rejects_odd_length_payload() {
        assert_eq!(validate_payload_hex("22F19"), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn rejects_non_hexadecimal_characters() {
        assert_eq!(validate_payload_hex("22G190"), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn accepts_maximum_payload() {
        let payload = "AA".repeat(4096);
        assert_eq!(validate_payload_hex(&payload), Ok(payload));
    }

    #[test]
    fn rejects_input_over_limit() {
        let payload = " ".repeat(8193);
        assert_eq!(
            validate_payload_hex(&payload),
            Err(StatusCode::PAYLOAD_TOO_LARGE)
        );
    }
}

#[cfg(test)]
mod raw_uds_policy_tests {
    use super::check_raw_uds_policy;
    use axum::http::StatusCode;

    #[test]
    fn allows_read_only_services() {
        for payload in ["22F190", "1902FF", "3E00", "3E80", "1001", "1003", "1083"] {
            assert_eq!(check_raw_uds_policy(payload), Ok(()), "{payload}");
        }
    }

    #[test]
    fn refuses_write_reset_routine_security_and_transfer() {
        for payload in [
            "2EF19001",
            "1101",
            "3101FF00",
            "2701",
            "2901",
            "280300",
            "8502",
            "14FFFFFF",
            "3400441000000000000100",
            "3601",
            "37",
        ] {
            assert_eq!(
                check_raw_uds_policy(payload),
                Err(StatusCode::FORBIDDEN),
                "{payload}"
            );
        }
    }

    #[test]
    fn refuses_programming_and_unknown_sessions() {
        for payload in ["1002", "1082", "1040", "1060"] {
            assert_eq!(
                check_raw_uds_policy(payload),
                Err(StatusCode::FORBIDDEN),
                "{payload}"
            );
        }
    }

    #[test]
    fn session_control_without_subfunction_is_bad_request() {
        assert_eq!(check_raw_uds_policy("10"), Err(StatusCode::BAD_REQUEST));
    }
}
