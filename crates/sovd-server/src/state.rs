//! Shared application state and ECU address resolution.

use crate::execution::manager::ExecutionManager;
use axum::http::StatusCode;
use reqwest::Client;
use std::sync::Arc;
use tokio::sync::OnceCell;
use vehicle_topology::{Ecu, VehicleTopology};

pub(crate) const EXECUTION_HISTORY_CAPACITY: usize = 128;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) client: Client,
    pub(crate) topology: VehicleTopology,
    pub(crate) zonal_http_port: u16,
    pub(crate) zonal_scheme: &'static str,
    pub(crate) execution_manager: Arc<OnceCell<ExecutionManager>>,
}

pub(crate) fn resolve_ecu_url(state: &AppState, ecu_id: &str) -> Result<String, StatusCode> {
    let ecu: &Ecu = state
        .topology
        .ecus
        .iter()
        .find(|ecu| ecu.id == ecu_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let zone = state
        .topology
        .zones
        .iter()
        .find(|zone| zone.id == ecu.zone)
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(format!(
        "{}://{}:{}/ecus/{}",
        state.zonal_scheme, zone.zonal_ip, state.zonal_http_port, ecu.id
    ))
}
