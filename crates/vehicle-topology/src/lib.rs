use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    net::IpAddr,
    path::Path,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VehicleTopology {
    pub vehicle: Vehicle,
    pub central: Central,
    pub zones: Vec<Zone>,
    pub ecus: Vec<Ecu>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Vehicle {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Central {
    pub namespace: String,
    pub sovd_bind: String,
    pub sovd_port: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Zone {
    pub id: String,
    pub name: String,
    pub namespace: String,
    pub can_interface: String,
    pub central_ip: String,
    pub zonal_ip: String,
    pub doip_port: u16,
    pub doip_logical_address: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Ecu {
    pub id: String,
    pub name: String,
    pub domain: String,
    pub zone: String,
    pub doip_logical_address: u16,
    pub can_interface: String,
    pub can_request_id: u16,
    pub can_response_id: u16,
}

impl VehicleTopology {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let contents = fs::read_to_string(path)
            .map_err(|error| format!("Cannot read topology file: {error}"))?;

        let topology: Self =
            toml::from_str(&contents).map_err(|error| format!("Invalid topology TOML: {error}"))?;

        topology.validate()?;
        Ok(topology)
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut errors = Vec::new();

        if self.vehicle.id.trim().is_empty() {
            errors.push("Vehicle ID must not be empty".to_string());
        }

        if self.central.namespace.trim().is_empty() {
            errors.push("Central namespace must not be empty".to_string());
        }

        if self.central.sovd_port == 0 {
            errors.push("Central SOVD port must not be zero".to_string());
        }

        if self.central.sovd_bind.parse::<IpAddr>().is_err() && self.central.sovd_bind != "0.0.0.0"
        {
            errors.push("Central SOVD bind address is invalid".to_string());
        }

        let mut zone_ids = HashSet::new();
        let mut namespaces = HashSet::new();
        let mut logical_addresses = HashSet::new();

        namespaces.insert(self.central.namespace.as_str());

        for zone in &self.zones {
            if zone.id.trim().is_empty() || !zone_ids.insert(zone.id.as_str()) {
                errors.push(format!("Empty or duplicate zone ID: {}", zone.id));
            }

            if !namespaces.insert(zone.namespace.as_str()) {
                errors.push(format!("Duplicate namespace: {}", zone.namespace));
            }

            if zone.can_interface.trim().is_empty() {
                errors.push(format!("Zone {} has no CAN interface", zone.id));
            }

            if zone.central_ip.parse::<IpAddr>().is_err()
                || zone.zonal_ip.parse::<IpAddr>().is_err()
            {
                errors.push(format!("Zone {} has an invalid IP address", zone.id));
            }

            if zone.doip_port == 0 {
                errors.push(format!("Zone {} has an invalid DoIP port", zone.id));
            }

            if !logical_addresses.insert(zone.doip_logical_address) {
                errors.push(format!(
                    "Duplicate DoIP logical address: 0x{:04X}",
                    zone.doip_logical_address
                ));
            }
        }

        let zone_interfaces: HashMap<&str, &str> = self
            .zones
            .iter()
            .map(|zone| (zone.id.as_str(), zone.can_interface.as_str()))
            .collect();

        let mut ecu_ids = HashSet::new();
        let mut can_ids = HashSet::new();

        for ecu in &self.ecus {
            if ecu.id.trim().is_empty() || !ecu_ids.insert(ecu.id.as_str()) {
                errors.push(format!("Empty or duplicate ECU ID: {}", ecu.id));
            }

            match zone_interfaces.get(ecu.zone.as_str()) {
                None => errors.push(format!(
                    "ECU {} references unknown zone {}",
                    ecu.id, ecu.zone
                )),
                Some(interface) if *interface != ecu.can_interface => errors.push(format!(
                    "ECU {} uses {}, but zone {} uses {}",
                    ecu.id, ecu.can_interface, ecu.zone, interface
                )),
                Some(_) => {}
            }

            if !logical_addresses.insert(ecu.doip_logical_address) {
                errors.push(format!(
                    "Duplicate DoIP logical address: 0x{:04X}",
                    ecu.doip_logical_address
                ));
            }

            for can_id in [ecu.can_request_id, ecu.can_response_id] {
                if can_id > 0x7FF {
                    errors.push(format!(
                        "ECU {} has CAN ID 0x{:X} outside 11-bit range",
                        ecu.id, can_id
                    ));
                }

                if !can_ids.insert((ecu.can_interface.as_str(), can_id)) {
                    errors.push(format!(
                        "Duplicate CAN ID 0x{:03X} on {}",
                        can_id, ecu.can_interface
                    ));
                }
            }

            if ecu.can_request_id == ecu.can_response_id {
                errors.push(format!(
                    "ECU {} request and response CAN IDs must differ",
                    ecu.id
                ));
            }
        }

        if self.zones.is_empty() {
            errors.push("At least one zone is required".to_string());
        }

        if self.ecus.is_empty() {
            errors.push("At least one ECU is required".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_vehicle_topology_is_valid() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../platform/config/vehicle-topology.toml"
        );

        let topology =
            VehicleTopology::load(path).expect("vehicle topology should load and validate");

        assert_eq!(topology.zones.len(), 2);
        assert_eq!(topology.ecus.len(), 5);
    }

    #[test]
    fn rejects_invalid_can_identifier() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../platform/config/vehicle-topology.toml"
        );

        let mut topology = VehicleTopology::load(path).unwrap();
        topology.ecus[0].can_request_id = 0x800;

        assert!(topology.validate().is_err());
    }
}
