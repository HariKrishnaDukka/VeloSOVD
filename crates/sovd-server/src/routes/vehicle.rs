//! Vehicle, topology, health and ECU discovery handlers.

use super::*;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use diagnostic_common::{DiagnosticValue, EcuInfo};

pub(crate) async fn get_topology(State(state): State<AppState>) -> Json<serde_json::Value> {
    let topology = &state.topology;

    Json(serde_json::json!({
        "vehicle": {
            "id": topology.vehicle.id,
            "name": topology.vehicle.name
        },
        "central": {
            "namespace": topology.central.namespace,
            "sovd_bind": topology.central.sovd_bind,
            "sovd_port": topology.central.sovd_port
        },
        "zones": topology.zones.iter().map(|zone| serde_json::json!({
            "id": zone.id,
            "name": zone.name,
            "namespace": zone.namespace,
            "can_interface": zone.can_interface,
            "central_ip": zone.central_ip,
            "zonal_ip": zone.zonal_ip,
            "doip_port": zone.doip_port,
            "doip_logical_address": zone.doip_logical_address
        })).collect::<Vec<_>>(),
        "ecus": topology.ecus.iter().map(|ecu| serde_json::json!({
            "id": ecu.id,
            "name": ecu.name,
            "domain": ecu.domain,
            "zone": ecu.zone,
            "doip_logical_address": ecu.doip_logical_address,
            "can_interface": ecu.can_interface,
            "can_request_id": ecu.can_request_id,
            "can_response_id": ecu.can_response_id
        })).collect::<Vec<_>>()
    }))
}

pub(crate) async fn health(State(state): State<AppState>) -> Json<VehicleHealth> {
    let mut zones = Vec::with_capacity(state.topology.zones.len());

    for zone in &state.topology.zones {
        let endpoint = format!(
            "{}://{}:{}/ecus",
            state.zonal_scheme, zone.zonal_ip, state.zonal_http_port
        );
        let probe = state.client.get(&endpoint).send().await;

        let (online, detail) = match probe {
            Ok(response) if response.status().is_success() => (
                true,
                format!(
                    "{} {}",
                    state.zonal_scheme.to_ascii_uppercase(),
                    response.status().as_u16()
                ),
            ),
            Ok(response) => (
                false,
                format!(
                    "{} {}",
                    state.zonal_scheme.to_ascii_uppercase(),
                    response.status().as_u16()
                ),
            ),
            Err(error) => (false, error.to_string()),
        };

        zones.push(ZoneHealth {
            id: zone.id.clone(),
            name: zone.name.clone(),
            online,
            detail,
        });
    }

    let overall = if zones.iter().all(|zone| zone.online) {
        "online"
    } else if zones.iter().any(|zone| zone.online) {
        "degraded"
    } else {
        "offline"
    };

    Json(VehicleHealth {
        vehicle_id: state.topology.vehicle.id.clone(),
        vehicle_name: state.topology.vehicle.name.clone(),
        overall: overall.to_string(),
        zones,
    })
}

pub(crate) async fn list_ecus(
    State(state): State<AppState>,
) -> Result<Json<Vec<EcuInfo>>, StatusCode> {
    let mut result = Vec::new();

    for zone in &state.topology.zones {
        let url = format!(
            "{}://{}:{}/ecus",
            state.zonal_scheme, zone.zonal_ip, state.zonal_http_port
        );

        let response = state
            .client
            .get(url)
            .send()
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?
            .error_for_status()
            .map_err(|_| StatusCode::BAD_GATEWAY)?;

        let mut zone_ecus = response
            .json::<Vec<EcuInfo>>()
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?;

        result.append(&mut zone_ecus);
    }

    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Json(result))
}

pub(crate) async fn ecu_info(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<EcuInfo>, StatusCode> {
    let url = resolve_ecu_url(&state, &id)?;

    let response = state
        .client
        .get(url)
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?
        .error_for_status()
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let info = response
        .json::<EcuInfo>()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    Ok(Json(info))
}

pub(crate) async fn ecu_resources(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ResourceDescriptor>>, StatusCode> {
    let _ = resolve_ecu_url(&state, &id)?;
    let base = format!("/api/ecus/{id}");

    let resources = vec![
        ResourceDescriptor {
            id: "vehicle-speed".to_string(),
            path: format!("{base}/data/vehicle-speed"),
            kind: "data".to_string(),
            description: "Vehicle speed value in km/h".to_string(),
            methods: vec!["GET".to_string()],
            simulated: true,
        },
        ResourceDescriptor {
            id: "faults".to_string(),
            path: format!("{base}/diagnostics/faults"),
            kind: "faults".to_string(),
            description: "Diagnostic trouble codes reported by this ECU".to_string(),
            methods: vec!["GET".to_string(), "DELETE".to_string()],
            simulated: true,
        },
        ResourceDescriptor {
            id: "uds-diagnostic-request".to_string(),
            path: format!("{base}/diagnostics/requests"),
            kind: "diagnostic-request".to_string(),
            description: "Submit a raw UDS diagnostic request as hexadecimal bytes".to_string(),
            methods: vec!["POST".to_string()],
            simulated: true,
        },
        ResourceDescriptor {
            id: "clear-faults".to_string(),
            path: format!("{base}/diagnostics/operations/clear-faults"),
            kind: "operation".to_string(),
            description: "Clear the ECU's simulated diagnostic fault memory".to_string(),
            methods: vec!["POST".to_string()],
            simulated: true,
        },
    ];

    Ok(Json(resources))
}

pub(crate) async fn ecu_operations(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<OperationDescriptor>>, StatusCode> {
    let _ = resolve_ecu_url(&state, &id)?;

    Ok(Json(vec![OperationDescriptor {
        id: "clear-faults".to_string(),
        name: "Clear fault memory".to_string(),
        method: "POST".to_string(),
        execution_path: format!("/api/ecus/{id}/operations/clear-faults/executions"),
        description: "Clear all stored diagnostic faults for this ECU".to_string(),
        supports_execution: true,
    }]))
}

pub(crate) async fn ecu_operation(
    State(state): State<AppState>,
    Path((id, operation_id)): Path<(String, String)>,
) -> Result<Json<OperationDescriptor>, StatusCode> {
    let _ = resolve_ecu_url(&state, &id)?;

    match operation_id.as_str() {
        "clear-faults" => Ok(Json(OperationDescriptor {
            id: "clear-faults".to_string(),
            name: "Clear fault memory".to_string(),
            method: "POST".to_string(),
            execution_path: format!("/api/ecus/{id}/operations/clear-faults/executions"),
            description: "Clear all stored diagnostic faults for this ECU".to_string(),
            supports_execution: true,
        })),
        _ => Err(StatusCode::NOT_FOUND),
    }
}

pub(crate) async fn vehicle_speed(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DiagnosticValue>, StatusCode> {
    let url = resolve_ecu_url(&state, &id)?;

    let response = state
        .client
        .get(format!("{url}/data/vehicle-speed"))
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?
        .error_for_status()
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let value = response
        .json::<DiagnosticValue>()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    Ok(Json(value))
}
