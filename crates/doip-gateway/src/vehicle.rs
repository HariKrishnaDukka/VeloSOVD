//! DoIP vehicle identification payload structures.

use std::fmt;

pub const VIN_LENGTH: usize = 17;
pub const EID_LENGTH: usize = 6;
pub const GID_LENGTH: usize = 6;
pub const VEHICLE_ANNOUNCEMENT_LENGTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VehiclePayloadError {
    InvalidLength,
}

impl fmt::Display for VehiclePayloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength => write!(f, "Invalid vehicle identification payload length"),
        }
    }
}

impl std::error::Error for VehiclePayloadError {}

/// Request for vehicle identification without additional data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VehicleIdentificationRequest;

/// Request filtered by a six-byte EID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VehicleIdentificationRequestWithEid {
    pub eid: [u8; EID_LENGTH],
}

/// Request filtered by a 17-byte VIN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VehicleIdentificationRequestWithVin {
    pub vin: [u8; VIN_LENGTH],
}

/// DoIP vehicle announcement / identification response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VehicleAnnouncement {
    pub vin: [u8; VIN_LENGTH],
    pub logical_address: u16,
    pub eid: [u8; EID_LENGTH],
    pub gid: [u8; GID_LENGTH],
    pub further_action: u8,
}

impl VehicleIdentificationRequestWithEid {
    pub fn decode(payload: &[u8]) -> Result<Self, VehiclePayloadError> {
        if payload.len() != EID_LENGTH {
            return Err(VehiclePayloadError::InvalidLength);
        }

        let mut eid = [0_u8; EID_LENGTH];
        eid.copy_from_slice(payload);

        Ok(Self { eid })
    }
}

impl VehicleIdentificationRequestWithVin {
    pub fn decode(payload: &[u8]) -> Result<Self, VehiclePayloadError> {
        if payload.len() != VIN_LENGTH {
            return Err(VehiclePayloadError::InvalidLength);
        }

        let mut vin = [0_u8; VIN_LENGTH];
        vin.copy_from_slice(payload);

        Ok(Self { vin })
    }
}

impl VehicleAnnouncement {
    /// Encodes the fixed-length vehicle announcement payload.
    pub fn encode(&self) -> [u8; VEHICLE_ANNOUNCEMENT_LENGTH] {
        let mut payload = [0_u8; VEHICLE_ANNOUNCEMENT_LENGTH];

        payload[0..17].copy_from_slice(&self.vin);
        payload[17..19].copy_from_slice(&self.logical_address.to_be_bytes());
        payload[19..25].copy_from_slice(&self.eid);
        payload[25..31].copy_from_slice(&self.gid);
        payload[31] = self.further_action;

        payload
    }

    /// Decodes a fixed-length vehicle announcement payload.
    pub fn decode(payload: &[u8]) -> Result<Self, VehiclePayloadError> {
        if payload.len() != VEHICLE_ANNOUNCEMENT_LENGTH {
            return Err(VehiclePayloadError::InvalidLength);
        }

        let mut vin = [0_u8; VIN_LENGTH];
        vin.copy_from_slice(&payload[0..17]);

        let logical_address = u16::from_be_bytes([payload[17], payload[18]]);

        let mut eid = [0_u8; EID_LENGTH];
        eid.copy_from_slice(&payload[19..25]);

        let mut gid = [0_u8; GID_LENGTH];
        gid.copy_from_slice(&payload[25..31]);

        Ok(Self {
            vin,
            logical_address,
            eid,
            gid,
            further_action: payload[31],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_eid_request() {
        let payload = [1, 2, 3, 4, 5, 6];
        let request = VehicleIdentificationRequestWithEid::decode(&payload).unwrap();

        assert_eq!(request.eid, payload);
    }

    #[test]
    fn rejects_invalid_eid_length() {
        assert_eq!(
            VehicleIdentificationRequestWithEid::decode(&[1, 2, 3]),
            Err(VehiclePayloadError::InvalidLength)
        );
    }

    #[test]
    fn decodes_vin_request() {
        let payload = *b"SDVDEMO0000000001";
        let request = VehicleIdentificationRequestWithVin::decode(&payload).unwrap();

        assert_eq!(request.vin, payload);
    }

    #[test]
    fn rejects_invalid_vin_length() {
        assert_eq!(
            VehicleIdentificationRequestWithVin::decode(&[0; 16]),
            Err(VehiclePayloadError::InvalidLength)
        );
    }

    #[test]
    fn announcement_round_trip() {
        let announcement = VehicleAnnouncement {
            vin: *b"SDVDEMO0000000001",
            logical_address: 0x1101,
            eid: [1, 2, 3, 4, 5, 6],
            gid: [6, 5, 4, 3, 2, 1],
            further_action: 0,
        };

        let encoded = announcement.encode();
        let decoded = VehicleAnnouncement::decode(&encoded).unwrap();

        assert_eq!(decoded, announcement);
    }

    #[test]
    fn rejects_invalid_announcement_length() {
        assert_eq!(
            VehicleAnnouncement::decode(&[0; 31]),
            Err(VehiclePayloadError::InvalidLength)
        );
    }
}
