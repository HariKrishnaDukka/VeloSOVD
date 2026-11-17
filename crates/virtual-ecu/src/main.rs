mod doip;
mod isotp;
mod uds_dispatcher;

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use diagnostic_common::{DiagnosticFault, DiagnosticValue, EcuInfo, OperationResult};
use doip_gateway::{DiagnosticRouter, RouterError, UdsDispatchFailure, UdsRequestHandler};
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, watch};
use uds_dispatcher::{
    DEFAULT_UDS_QUEUE_CAPACITY, UdsDispatchError, UdsDispatcherHandle, spawn_uds_dispatcher,
};
use uds_protocol::{DidEntry, DtcEntry, DtcStore, UdsServer, UdsSession, new_dtc_store};
use vehicle_topology::{Ecu, VehicleTopology};

#[derive(Clone)]
struct EcuState {
    info: EcuInfo,
    speed: f64,
    dtcs: DtcStore,
}

type EcuRegistry = Arc<RwLock<HashMap<String, EcuState>>>;

/// DoIP ECU logical address -> shared UDS dispatcher.
type DoipEcuRegistry = Arc<HashMap<u16, UdsDispatcherHandle>>;
type DiagnosticDispatcherRegistry = Arc<HashMap<String, UdsDispatcherHandle>>;

#[derive(Debug, Deserialize)]
struct UdsDiagnosticRequest {
    payload_hex: String,
}

#[derive(Debug, Serialize)]
struct UdsDiagnosticResponse {
    ecu_id: String,
    request_hex: String,
    response_hex: Option<String>,
}

fn decode_hex_payload(input: &str) -> Result<Vec<u8>, StatusCode> {
    if input.len() > 8192 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    let compact: String = input
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect();

    if compact.is_empty() || !compact.len().is_multiple_of(2) {
        return Err(StatusCode::BAD_REQUEST);
    }

    if compact.len() > 8192 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    let mut payload = Vec::with_capacity(compact.len() / 2);
    for pair in compact.as_bytes().as_chunks::<2>().0 {
        let pair = std::str::from_utf8(pair).map_err(|_| StatusCode::BAD_REQUEST)?;
        let byte = u8::from_str_radix(pair, 16).map_err(|_| StatusCode::BAD_REQUEST)?;
        payload.push(byte);
    }

    Ok(payload)
}

fn encode_hex_payload(payload: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::with_capacity(payload.len() * 2);

    for byte in payload {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0F) as usize] as char);
    }

    output
}

/// Register a dispatcher without silently overwriting an existing address.
fn register_doip_dispatcher(
    registry: &mut HashMap<u16, UdsDispatcherHandle>,
    logical_address: u16,
    dispatcher: UdsDispatcherHandle,
) -> Result<(), String> {
    match registry.entry(logical_address) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(dispatcher);
            Ok(())
        }
        std::collections::hash_map::Entry::Occupied(_) => Err(format!(
            "Duplicate ECU DoIP logical address: 0x{logical_address:04X}"
        )),
    }
}

/// Adapter connecting the DoIP router to the ECU's shared UDS dispatcher.
struct DispatcherUdsHandler {
    dispatcher: UdsDispatcherHandle,
}

impl UdsRequestHandler for DispatcherUdsHandler {
    fn process_uds(&self, payload: &[u8]) -> Result<Option<Vec<u8>>, UdsDispatchFailure> {
        self.dispatcher
            .process_blocking(payload.to_vec())
            .map_err(|error| match error {
                UdsDispatchError::QueueFull => UdsDispatchFailure::QueueFull,
                UdsDispatchError::ActorUnavailable => UdsDispatchFailure::ActorUnavailable,
                UdsDispatchError::ResponseUnavailable => UdsDispatchFailure::ResponseUnavailable,
                UdsDispatchError::ResponseTimeout => UdsDispatchFailure::ResponseTimeout,
            })
    }
}

/// Build the DoIP router from the already validated ECU dispatcher registry.
fn build_doip_router(registry: &DoipEcuRegistry) -> Result<DiagnosticRouter, RouterError> {
    let mut router = DiagnosticRouter::new();

    for (&logical_address, dispatcher) in registry.iter() {
        router.register(
            logical_address,
            Arc::new(DispatcherUdsHandler {
                dispatcher: dispatcher.clone(),
            }),
        )?;
    }

    Ok(router)
}

async fn diagnostic_request(
    State(dispatchers): State<DiagnosticDispatcherRegistry>,
    Path(id): Path<String>,
    Json(request): Json<UdsDiagnosticRequest>,
) -> Result<Json<UdsDiagnosticResponse>, StatusCode> {
    let payload = decode_hex_payload(&request.payload_hex)?;
    if payload.len() > 4096 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    let dispatcher = dispatchers.get(&id).cloned().ok_or(StatusCode::NOT_FOUND)?;
    let request_hex = encode_hex_payload(&payload);

    // The dispatcher uses a blocking standard channel. Keep that wait off
    // the Tokio executor so other ECU HTTP requests remain schedulable.
    let result = tokio::task::spawn_blocking(move || dispatcher.process_blocking(payload))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let response = result.map_err(|error| match error {
        UdsDispatchError::QueueFull => StatusCode::SERVICE_UNAVAILABLE,
        UdsDispatchError::ActorUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        UdsDispatchError::ResponseUnavailable => StatusCode::BAD_GATEWAY,
        UdsDispatchError::ResponseTimeout => StatusCode::GATEWAY_TIMEOUT,
    })?;

    Ok(Json(UdsDiagnosticResponse {
        ecu_id: id,
        request_hex,
        response_hex: response.as_deref().map(encode_hex_payload),
    }))
}

async fn list_ecus(State(registry): State<EcuRegistry>) -> Json<Vec<EcuInfo>> {
    let ecus = registry.read().await;
    let mut result: Vec<EcuInfo> = ecus.values().map(|ecu| ecu.info.clone()).collect();

    // Keep API output deterministic regardless of HashMap iteration order.
    result.sort_by(|a, b| a.id.cmp(&b.id));

    Json(result)
}

async fn ecu_info(
    State(registry): State<EcuRegistry>,
    Path(id): Path<String>,
) -> Result<Json<EcuInfo>, StatusCode> {
    let ecus = registry.read().await;
    let ecu = ecus.get(&id).ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(ecu.info.clone()))
}

async fn vehicle_speed(
    State(registry): State<EcuRegistry>,
    Path(id): Path<String>,
) -> Result<Json<DiagnosticValue>, StatusCode> {
    let ecus = registry.read().await;
    let ecu = ecus.get(&id).ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(DiagnosticValue {
        name: "vehicle_speed".to_string(),
        value: ecu.speed,
        unit: "km/h".to_string(),
    }))
}

async fn read_faults(
    State(registry): State<EcuRegistry>,
    Path(id): Path<String>,
) -> Result<Json<Vec<DiagnosticFault>>, StatusCode> {
    let ecus = registry.read().await;
    let ecu = ecus.get(&id).ok_or(StatusCode::NOT_FOUND)?;

    let dtcs = ecu
        .dtcs
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let faults = dtcs
        .iter()
        .map(|dtc| DiagnosticFault {
            code: dtc.code.clone(),
            description: dtc.description.clone(),
            active: dtc.status & 0x01 != 0,
            status: dtc.status,
        })
        .collect();
    Ok(Json(faults))
}

async fn read_fault(
    State(registry): State<EcuRegistry>,
    Path((id, fault_code)): Path<(String, String)>,
) -> Result<Json<DiagnosticFault>, StatusCode> {
    let ecus = registry.read().await;
    let ecu = ecus.get(&id).ok_or(StatusCode::NOT_FOUND)?;

    let dtcs = ecu
        .dtcs
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let fault = dtcs
        .iter()
        .find(|dtc| dtc.code == fault_code)
        .map(|dtc| DiagnosticFault {
            code: dtc.code.clone(),
            description: dtc.description.clone(),
            active: dtc.status & 0x01 != 0,
            status: dtc.status,
        })
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(fault))
}

async fn delete_faults(
    State(registry): State<EcuRegistry>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let ecus = registry.read().await;
    let ecu = ecus.get(&id).ok_or(StatusCode::NOT_FOUND)?;

    let mut dtcs = ecu
        .dtcs
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    dtcs.clear();

    Ok(StatusCode::NO_CONTENT)
}

async fn delete_fault(
    State(registry): State<EcuRegistry>,
    Path((id, fault_code)): Path<(String, String)>,
) -> Result<StatusCode, StatusCode> {
    let ecus = registry.read().await;
    let ecu = ecus.get(&id).ok_or(StatusCode::NOT_FOUND)?;

    let mut dtcs = ecu
        .dtcs
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let index = dtcs
        .iter()
        .position(|dtc| dtc.code == fault_code)
        .ok_or(StatusCode::NOT_FOUND)?;

    dtcs.remove(index);

    Ok(StatusCode::NO_CONTENT)
}

async fn clear_faults(
    State(registry): State<EcuRegistry>,
    Path(id): Path<String>,
) -> Result<Json<OperationResult>, StatusCode> {
    let mut ecus = registry.write().await;
    let ecu = ecus.get_mut(&id).ok_or(StatusCode::NOT_FOUND)?;

    let mut dtcs = ecu
        .dtcs
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    dtcs.clear();

    Ok(Json(OperationResult {
        operation: "clear_faults".to_string(),
        success: true,
        message: format!("{} fault memory cleared", id),
    }))
}

fn make_ecu(
    id: &str,
    name: &str,
    domain: &str,
    speed: f64,
    fault: Option<(&str, &str)>,
) -> EcuState {
    let dtcs = fault
        .map(|(code, description)| {
            vec![DtcEntry {
                dtc: dtc_code_to_u24(code).unwrap_or(0),
                code: code.to_string(),
                description: description.to_string(),
                // testFailed + confirmedDTC for this simulated active fault.
                status: 0x09,
            }]
        })
        .unwrap_or_default();

    EcuState {
        info: EcuInfo {
            id: id.to_string(),
            name: name.to_string(),
            domain: domain.to_string(),
            status: "online".to_string(),
        },
        speed,
        dtcs: new_dtc_store(dtcs),
    }
}

/// Build the HTTP-visible state using the topology as the ECU inventory.
/// The sample values and faults are currently simulated.
fn make_ecu_from_topology(ecu: &Ecu) -> EcuState {
    let (speed, fault) = match ecu.id.as_str() {
        "ECM" => (
            65.0,
            Some(("P0128", "Coolant thermostat temperature below threshold")),
        ),
        "BCM" => (0.0, Some(("B1000", "Body control configuration fault"))),
        _ => (0.0, None),
    };

    make_ecu(&ecu.id, &ecu.name, &ecu.domain, speed, fault)
}

/// Convert a five-character SAE DTC (for example P0128) to its 24-bit value.
fn dtc_code_to_u24(code: &str) -> Option<u32> {
    let bytes = code.as_bytes();
    if bytes.len() != 5 {
        return None;
    }
    let system = match bytes[0] {
        b'P' => 0_u32,
        b'C' => 1_u32,
        b'B' => 2_u32,
        b'U' => 3_u32,
        _ => return None,
    };
    let digits = std::str::from_utf8(&bytes[1..]).ok()?;
    if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let second_digit = (digits.as_bytes()[1] - b'0') as u32;
    let numeric_code = u32::from_str_radix(digits, 16).ok()?;
    Some((system << 22) | (second_digit << 16) | numeric_code)
}

/// Each ECU has an independent UDS session and a shared HTTP/UDS DTC store.
fn make_uds_server(ecu_id: &str, dtcs: DtcStore) -> UdsServer {
    let mut server = UdsServer::with_dtc_store(dtcs);

    server.register_did(DidEntry {
        identifier: 0xF190,
        data: b"SDVDEMO0000000001".to_vec(),
        allowed_sessions: vec![UdsSession::Default, UdsSession::Extended],
    });

    let software_version = format!("{ecu_id}-SOFTWARE-001");

    server.register_did(DidEntry {
        identifier: 0xF187,
        data: software_version.into_bytes(),
        allowed_sessions: vec![UdsSession::Extended],
    });

    server
}

#[tokio::main]
async fn main() {
    // Load the topology file. SDV_TOPOLOGY can override the default path.
    let topology_path = std::env::var("SDV_TOPOLOGY")
        .unwrap_or_else(|_| "platform/config/vehicle-topology.toml".to_string());

    let topology = VehicleTopology::load(&topology_path).unwrap_or_else(|error| {
        panic!("Failed to load topology '{}': {}", topology_path, error);
    });

    println!(
        "[TOPOLOGY] Loaded vehicle '{}' ({})",
        topology.vehicle.name, topology.vehicle.id
    );
    println!(
        "[TOPOLOGY] Configured zones: {}, ECUs: {}",
        topology.zones.len(),
        topology.ecus.len()
    );

    // Each runtime instance is restricted to one Linux network namespace/zone.
    let selected_zone = std::env::var("SDV_ZONE").unwrap_or_else(|_| {
        panic!("SDV_ZONE must be set to FRONT or REAR");
    });

    let selected_zone_config = topology
        .zones
        .iter()
        .find(|zone| zone.id.eq_ignore_ascii_case(&selected_zone))
        .cloned()
        .unwrap_or_else(|| panic!("Unknown SDV_ZONE '{}'", selected_zone));

    let selected_ecus: Vec<Ecu> = topology
        .ecus
        .iter()
        .filter(|ecu| ecu.zone.eq_ignore_ascii_case(&selected_zone))
        .cloned()
        .collect();

    if selected_ecus.is_empty() {
        panic!(
            "No ECUs configured for zone '{}'. Check SDV_ZONE and vehicle topology.",
            selected_zone
        );
    }

    println!(
        "[ZONE] Runtime zone: {} | ECU count: {}",
        selected_zone,
        selected_ecus.len()
    );

    // Build the HTTP ECU registry using only this zone's ECUs.
    let mut initial_ecus = HashMap::new();

    for ecu in &selected_ecus {
        println!(
            "[TOPOLOGY] ECU={} ZONE={} CAN={} RX=0x{:03X} TX=0x{:03X}",
            ecu.id, ecu.zone, ecu.can_interface, ecu.can_request_id, ecu.can_response_id
        );

        initial_ecus.insert(ecu.id.clone(), make_ecu_from_topology(ecu));
    }

    let registry: EcuRegistry = Arc::new(RwLock::new(initial_ecus));

    // Bind HTTP before starting transport threads. A bind failure should not
    // leave detached ISO-TP threads running.
    let bind_address =
        std::env::var("SDV_HTTP_BIND").unwrap_or_else(|_| "0.0.0.0:9001".to_string());

    let listener = tokio::net::TcpListener::bind(&bind_address)
        .await
        .unwrap_or_else(|error| {
            panic!("Cannot bind virtual ECU HTTP API at {bind_address}: {error}");
        });

    // Bind DoIP only to this zone's configured zonal address.
    let doip_bind_address = std::env::var("SDV_DOIP_BIND").unwrap_or_else(|_| {
        format!(
            "{}:{}",
            selected_zone_config.zonal_ip, selected_zone_config.doip_port
        )
    });

    let doip_listener = tokio::net::TcpListener::bind(&doip_bind_address)
        .await
        .unwrap_or_else(|error| {
            panic!("Cannot bind DoIP TCP listener at {doip_bind_address}: {error}");
        });

    // Each ECU has one UDS actor shared by its diagnostic transports.
    // The bounded queue prevents unbounded request accumulation.
    let running = Arc::new(AtomicBool::new(true));
    let mut isotp_threads = Vec::with_capacity(selected_ecus.len());
    let mut uds_actor_threads = Vec::with_capacity(selected_ecus.len());
    let mut doip_dispatchers = HashMap::with_capacity(selected_ecus.len());
    let mut diagnostic_dispatchers = HashMap::with_capacity(selected_ecus.len());

    for ecu in selected_ecus.iter().cloned() {
        let logical_address = ecu.doip_logical_address;
        let ecu_id = ecu.id;
        let interface = ecu.can_interface;
        let rx_id = u32::from(ecu.can_request_id);
        let tx_id = u32::from(ecu.can_response_id);

        let ecu_dtcs = {
            let ecus = registry.read().await;
            Arc::clone(&ecus.get(&ecu_id).expect("selected ECU must exist").dtcs)
        };
        let uds_server = make_uds_server(&ecu_id, ecu_dtcs);
        let (dispatcher, actor_thread) =
            spawn_uds_dispatcher(ecu_id.clone(), uds_server, DEFAULT_UDS_QUEUE_CAPACITY);
        uds_actor_threads.push((ecu_id.clone(), actor_thread));
        diagnostic_dispatchers.insert(ecu_id.clone(), dispatcher.clone());

        // Register the same UDS dispatcher for future DoIP routing.
        register_doip_dispatcher(&mut doip_dispatchers, logical_address, dispatcher.clone())
            .expect("validated topology must have unique ECU DoIP logical addresses");

        let thread_running = Arc::clone(&running);
        let thread_ecu_id = ecu_id.clone();
        let thread_dispatcher: UdsDispatcherHandle = dispatcher.clone();
        drop(dispatcher);

        let handle = std::thread::spawn(move || {
            if let Err(error) = isotp::run_isotp_endpoint(
                &thread_ecu_id,
                &interface,
                rx_id,
                tx_id,
                thread_dispatcher,
                thread_running,
            ) {
                eprintln!("[{thread_ecu_id}][ISO-TP] Transport stopped: {error}");
            }
        });

        isotp_threads.push((ecu_id, handle));
    }

    let doip_ecu_registry: DoipEcuRegistry = Arc::new(doip_dispatchers);
    let diagnostic_dispatcher_registry: DiagnosticDispatcherRegistry =
        Arc::new(diagnostic_dispatchers);

    let doip_router = build_doip_router(&doip_ecu_registry)
        .expect("validated ECU registry must produce a valid DoIP router");

    println!(
        "[DoIP] Prepared diagnostic router for {} ECU(s)",
        doip_router.target_count()
    );

    // Start the zone-specific DoIP listener using the same UDS dispatchers.
    let (doip_shutdown_tx, doip_shutdown_rx) = watch::channel(false);
    let doip_router = Arc::new(doip_router);
    let doip_entity_address = selected_zone_config.doip_logical_address;

    let doip_task = tokio::spawn(doip::run_listener(
        doip_listener,
        doip_entity_address,
        doip_router,
        doip_shutdown_rx,
    ));

    println!(
        "[DoIP] TCP listener active at {} | entity logical address 0x{:04X}",
        doip_bind_address, doip_entity_address
    );

    let diagnostic_router = Router::new()
        .route("/ecus/{id}/diagnostics/requests", post(diagnostic_request))
        .with_state(diagnostic_dispatcher_registry);

    let app = Router::new()
        .route("/ecus", get(list_ecus))
        .route("/ecus/{id}", get(ecu_info))
        .route("/ecus/{id}/data/vehicle-speed", get(vehicle_speed))
        .route(
            "/ecus/{id}/diagnostics/faults",
            get(read_faults).delete(delete_faults),
        )
        .route(
            "/ecus/{id}/diagnostics/faults/{fault_code}",
            get(read_fault).delete(delete_fault),
        )
        .route(
            "/ecus/{id}/diagnostics/operations/clear-faults",
            post(clear_faults),
        )
        .merge(diagnostic_router)
        .with_state(registry);

    println!("[HTTP] Virtual ECU API listening at http://{bind_address}");
    println!(
        "[ISO-TP] Started {} configured ECU endpoint(s)",
        isotp_threads.len()
    );

    let shutdown_running = Arc::clone(&running);
    let doip_shutdown_signal = doip_shutdown_tx.clone();

    let server_result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            println!("[SYSTEM] Shutdown requested");
            shutdown_running.store(false, Ordering::Release);
            let _ = doip_shutdown_signal.send(true);
        })
        .await;

    running.store(false, Ordering::Release);
    let _ = doip_shutdown_tx.send(true);

    // Stop DoIP connections before releasing dispatcher handles or joining actors.
    match doip_task.await {
        Ok(Ok(())) => println!("[DoIP] Listener and connection tasks stopped"),
        Ok(Err(error)) => eprintln!("[DoIP] Listener I/O error: {error}"),
        Err(error) => eprintln!("[DoIP] Listener task failed: {error}"),
    }

    drop(doip_ecu_registry);

    if let Err(error) = server_result {
        eprintln!("[HTTP] Server failed: {error}");
    }

    // ISO-TP workers drop their dispatcher handles when they exit. That
    // closes each actor queue; only then is it safe to join the actor threads.
    for (ecu_id, handle) in isotp_threads {
        match handle.join() {
            Ok(()) => println!("[{ecu_id}][ISO-TP] Thread stopped"),
            Err(_) => eprintln!("[{ecu_id}][ISO-TP] Thread panicked"),
        }
    }

    for (ecu_id, handle) in uds_actor_threads {
        match handle.join() {
            Ok(()) => println!("[{ecu_id}][UDS] Actor thread stopped"),
            Err(_) => eprintln!("[{ecu_id}][UDS] Actor thread panicked"),
        }
    }

    println!("[SYSTEM] Virtual ECU runtime stopped");
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_UDS_QUEUE_CAPACITY, HashMap, UdsServer, dtc_code_to_u24, register_doip_dispatcher,
        spawn_uds_dispatcher,
    };

    #[test]
    fn converts_sae_powertrain_dtc() {
        assert_eq!(dtc_code_to_u24("P0128"), Some(0x010128));
    }

    #[test]
    fn rejects_malformed_dtc() {
        assert_eq!(dtc_code_to_u24("P01"), None);
        assert_eq!(dtc_code_to_u24("X0128"), None);
    }

    #[test]
    fn registry_resolves_configured_logical_address() {
        let (dispatcher, actor) = spawn_uds_dispatcher(
            "REGISTRY_TEST".to_owned(),
            UdsServer::new(),
            DEFAULT_UDS_QUEUE_CAPACITY,
        );

        let mut registry = HashMap::new();

        assert!(register_doip_dispatcher(&mut registry, 0x1110, dispatcher.clone()).is_ok());

        let registered = registry.get(&0x1110).expect("registered ECU must exist");
        let response = registered.process_blocking(vec![0x99]).unwrap();

        assert_eq!(response, Some(vec![0x7F, 0x99, 0x11]));

        drop(registry);
        drop(dispatcher);
        actor.join().unwrap();
    }

    #[test]
    fn registry_rejects_unknown_logical_address() {
        let (dispatcher, actor) = spawn_uds_dispatcher(
            "REGISTRY_TEST".to_owned(),
            UdsServer::new(),
            DEFAULT_UDS_QUEUE_CAPACITY,
        );

        let mut registry = HashMap::new();

        register_doip_dispatcher(&mut registry, 0x1110, dispatcher.clone()).unwrap();

        assert!(!registry.contains_key(&0x9999));

        drop(registry);
        drop(dispatcher);
        actor.join().unwrap();
    }

    #[test]
    fn registry_rejects_duplicate_logical_address() {
        let (dispatcher, actor) = spawn_uds_dispatcher(
            "REGISTRY_TEST".to_owned(),
            UdsServer::new(),
            DEFAULT_UDS_QUEUE_CAPACITY,
        );

        let mut registry = HashMap::new();

        register_doip_dispatcher(&mut registry, 0x1110, dispatcher.clone()).unwrap();

        let duplicate_result = register_doip_dispatcher(&mut registry, 0x1110, dispatcher.clone());

        assert_eq!(
            duplicate_result,
            Err("Duplicate ECU DoIP logical address: 0x1110".to_owned())
        );
        assert_eq!(registry.len(), 1);

        drop(registry);
        drop(dispatcher);
        actor.join().unwrap();
    }
}

#[cfg(test)]
mod dtc_shared_state_integration_tests {
    use super::*;

    fn test_registry() -> EcuRegistry {
        let mut ecus = HashMap::new();
        ecus.insert(
            "ECM".to_string(),
            make_ecu(
                "ECM",
                "Engine Control Module",
                "powertrain",
                65.0,
                Some(("P0128", "Coolant thermostat temperature below threshold")),
            ),
        );

        Arc::new(RwLock::new(ecus))
    }

    #[tokio::test]
    async fn http_and_uds_read_the_same_dtc_store() {
        let registry = test_registry();

        // Obtain the exact DTC store owned by the HTTP ECU state.
        let dtcs = {
            let ecus = registry.read().await;
            ecus.get("ECM").unwrap().dtcs.clone()
        };

        let mut uds = make_uds_server("ECM", dtcs);

        // HTTP must report the initial fault.
        let http_response = read_faults(State(registry.clone()), Path("ECM".to_string()))
            .await
            .unwrap();

        assert_eq!(http_response.0.len(), 1);
        assert_eq!(http_response.0[0].code, "P0128");
        assert!(http_response.0[0].active);

        // UDS must report that same fault.
        let uds_response = uds.process_request(&[0x19, 0x02, 0x08]).unwrap();

        assert_eq!(uds_response, vec![0x59, 0x02, 0xFF, 0x01, 0x01, 0x28, 0x09]);
    }

    #[tokio::test]
    async fn http_reads_individual_fault_from_shared_dtc_store() {
        let registry = test_registry();

        // Retrieve the existing fault by its exact DTC code.
        let response = read_fault(
            State(registry.clone()),
            Path(("ECM".to_string(), "P0128".to_string())),
        )
        .await
        .expect("Existing DTC must be retrievable");

        assert_eq!(response.0.code, "P0128");
        assert_eq!(
            response.0.description,
            "Coolant thermostat temperature below threshold"
        );
        assert!(response.0.active);

        // An unknown DTC must return HTTP 404.
        let unknown_fault = read_fault(
            State(registry.clone()),
            Path(("ECM".to_string(), "P9999".to_string())),
        )
        .await;

        assert!(matches!(unknown_fault, Err(StatusCode::NOT_FOUND)));

        // An unknown ECU must also return HTTP 404.
        let unknown_ecu = read_fault(
            State(registry),
            Path(("UNKNOWN".to_string(), "P0128".to_string())),
        )
        .await;

        assert!(matches!(unknown_ecu, Err(StatusCode::NOT_FOUND)));
    }

    #[tokio::test]
    async fn http_delete_individual_fault_is_visible_through_uds() {
        let registry = test_registry();

        let dtcs = {
            let ecus = registry.read().await;
            ecus.get("ECM").unwrap().dtcs.clone()
        };

        let mut uds = make_uds_server("ECM", dtcs);

        let response = delete_fault(
            State(registry.clone()),
            Path(("ECM".to_string(), "P0128".to_string())),
        )
        .await
        .expect("Existing fault must be deleted");

        assert_eq!(response, StatusCode::NO_CONTENT);

        let http_response = read_faults(State(registry.clone()), Path("ECM".to_string()))
            .await
            .unwrap();

        assert!(http_response.0.is_empty());

        let uds_response = uds.process_request(&[0x19, 0x02, 0x08]).unwrap();

        assert_eq!(uds_response, vec![0x59, 0x02, 0xFF]);
    }

    #[tokio::test]
    async fn http_delete_individual_fault_preserves_other_faults() {
        let registry = test_registry();

        // Obtain the exact DTC store shared by HTTP and UDS.
        let dtcs = {
            let ecus = registry.read().await;
            ecus.get("ECM").expect("ECM must exist").dtcs.clone()
        };

        // Add a second DTC so that individual deletion can be verified.
        {
            let mut dtcs_guard = dtcs.lock().expect("DTC store lock must succeed");

            dtcs_guard.push(DtcEntry {
                dtc: dtc_code_to_u24("P0300").expect("P0300 must be a valid DTC code"),
                code: "P0300".to_string(),
                description: "Random/multiple cylinder misfire detected".to_string(),
                status: 0x09,
            });
        }

        // Create the UDS server using the same DTC store.
        let mut uds = make_uds_server("ECM", dtcs.clone());

        // Delete only P0128 through the HTTP handler.
        let response = delete_fault(
            State(registry.clone()),
            Path(("ECM".to_string(), "P0128".to_string())),
        )
        .await
        .expect("Existing fault must be deleted");

        assert_eq!(response, StatusCode::NO_CONTENT);

        // Verify that HTTP can still retrieve the remaining P0300 fault.
        let http_response = read_faults(State(registry.clone()), Path("ECM".to_string()))
            .await
            .expect("Remaining faults must be readable");

        assert_eq!(http_response.0.len(), 1);
        assert_eq!(http_response.0[0].code, "P0300");
        assert_eq!(
            http_response.0[0].description,
            "Random/multiple cylinder misfire detected"
        );
        assert!(http_response.0[0].active);

        // Verify the shared DTC store contains exactly P0300.
        {
            let dtcs_guard = dtcs.lock().expect("DTC store lock must succeed");

            assert_eq!(dtcs_guard.len(), 1);
            assert_eq!(dtcs_guard[0].code, "P0300");
            assert_eq!(dtcs_guard[0].status, 0x09);
            assert_eq!(
                dtcs_guard[0].dtc,
                dtc_code_to_u24("P0300").expect("P0300 must be a valid DTC code")
            );
        }

        // Verify UDS ReadDTCInformation (0x19, subfunction 0x02).
        let uds_response = uds
            .process_request(&[0x19, 0x02, 0x08])
            .expect("UDS request must be processed");

        println!(
            "UDS response after individual deletion: {:02X?}",
            uds_response
        );

        let expected_dtc = dtc_code_to_u24("P0300").expect("P0300 must be a valid DTC code");

        let expected_response = vec![
            0x59,
            0x02,
            0xFF,
            (expected_dtc >> 16) as u8,
            (expected_dtc >> 8) as u8,
            expected_dtc as u8,
            0x09,
        ];

        assert_eq!(uds_response, expected_response);
    }

    #[tokio::test]
    async fn http_delete_fault_collection_clears_shared_dtc_store() {
        let registry = test_registry();

        let dtcs = {
            let ecus = registry.read().await;
            ecus.get("ECM").unwrap().dtcs.clone()
        };

        let mut uds = make_uds_server("ECM", dtcs);

        let response = delete_faults(State(registry.clone()), Path("ECM".to_string()))
            .await
            .expect("Fault collection must be deleted");

        assert_eq!(response, StatusCode::NO_CONTENT);

        let http_response = read_faults(State(registry.clone()), Path("ECM".to_string()))
            .await
            .unwrap();

        assert!(http_response.0.is_empty());

        let uds_response = uds.process_request(&[0x19, 0x02, 0x08]).unwrap();

        assert_eq!(uds_response, vec![0x59, 0x02, 0xFF]);
    }

    #[tokio::test]
    async fn http_delete_fault_returns_not_found_for_unknown_resources() {
        let registry = test_registry();

        let unknown_fault = delete_fault(
            State(registry.clone()),
            Path(("ECM".to_string(), "P9999".to_string())),
        )
        .await;

        assert!(matches!(unknown_fault, Err(StatusCode::NOT_FOUND)));

        let unknown_ecu = delete_fault(
            State(registry.clone()),
            Path(("UNKNOWN".to_string(), "P0128".to_string())),
        )
        .await;

        assert!(matches!(unknown_ecu, Err(StatusCode::NOT_FOUND)));

        let unknown_collection = delete_faults(State(registry), Path("UNKNOWN".to_string())).await;

        assert!(matches!(unknown_collection, Err(StatusCode::NOT_FOUND)));
    }

    #[tokio::test]
    async fn http_clear_is_visible_through_uds() {
        let registry = test_registry();

        let dtcs = {
            let ecus = registry.read().await;
            ecus.get("ECM").unwrap().dtcs.clone()
        };

        let mut uds = make_uds_server("ECM", dtcs);

        let clear_response = clear_faults(State(registry.clone()), Path("ECM".to_string()))
            .await
            .unwrap();

        assert!(clear_response.0.success);

        let uds_response = uds.process_request(&[0x19, 0x02, 0xFF]).unwrap();

        assert_eq!(uds_response, vec![0x59, 0x02, 0xFF]);
    }

    #[tokio::test]
    async fn uds_clear_is_visible_through_http() {
        let registry = test_registry();

        let dtcs = {
            let ecus = registry.read().await;
            ecus.get("ECM").unwrap().dtcs.clone()
        };

        let mut uds = make_uds_server("ECM", dtcs);

        let uds_clear_response = uds.process_request(&[0x14, 0xFF, 0xFF, 0xFF]).unwrap();

        assert_eq!(uds_clear_response, vec![0x54]);

        let http_response = read_faults(State(registry.clone()), Path("ECM".to_string()))
            .await
            .unwrap();

        assert!(http_response.0.is_empty());
    }
}

#[cfg(test)]
mod doip_real_uds_integration_tests {
    use super::*;
    use doip_gateway::{
        DiagnosticMessage, DoipTransport, ROUTING_RESPONSE_SUCCESS, RoutingActivationRequest,
        RoutingActivationResponse,
    };
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::{
        net::{TcpListener, TcpStream},
        sync::watch,
        time::{Duration as TokioDuration, timeout},
    };

    const TESTER_ADDRESS: u16 = 0x0E00;
    const ENTITY_ADDRESS: u16 = 0x1101;
    const ECU_ADDRESS: u16 = 0x1110;

    #[tokio::test]
    async fn doip_reads_vin_through_production_uds_dispatcher() {
        // Create the same UDS server used by the virtual ECU.
        let uds_server = make_uds_server("VIN_TEST", new_dtc_store(Vec::new()));

        // Run UDS processing on its dedicated blocking OS thread.
        let (dispatcher, actor) = spawn_uds_dispatcher(
            "VIN_TEST".to_owned(),
            uds_server,
            DEFAULT_UDS_QUEUE_CAPACITY,
        );

        // Register the real dispatcher with the production DoIP router.
        let registry = Arc::new(HashMap::from([(ECU_ADDRESS, dispatcher.clone())]));

        let router = Arc::new(build_doip_router(&registry).expect("failed to build DoIP router"));

        // Use an ephemeral loopback port to avoid port conflicts.
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind test DoIP listener");

        let server_address = listener
            .local_addr()
            .expect("failed to obtain listener address");

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let listener_router = Arc::clone(&router);

        let listener_task = tokio::spawn(async move {
            doip::run_listener(listener, ENTITY_ADDRESS, listener_router, shutdown_rx).await
        });

        // Keep the client exchange in its own scope so the TCP connection
        // is closed before the listener is shut down.
        async {
            let stream = TcpStream::connect(server_address)
                .await
                .expect("failed to connect DoIP test client");

            let mut client = DoipTransport::new(stream);

            // 1. Request DoIP routing activation.
            let activation_request = RoutingActivationRequest {
                tester_logical_address: TESTER_ADDRESS,
                activation_type: 0x00,
                reserved: [0; 4],
            };

            client
                .send_frame(0x0005, &activation_request.encode())
                .await
                .expect("failed to send routing activation");

            // 2. Verify routing activation response.
            let activation_frame = timeout(TokioDuration::from_secs(2), client.receive_frame())
                .await
                .expect("routing activation timed out")
                .expect("failed to receive routing activation response");

            assert_eq!(activation_frame.header.payload_type, 0x0006);

            let activation_response = RoutingActivationResponse::decode(&activation_frame.payload)
                .expect("invalid routing activation response");

            assert_eq!(activation_response.tester_logical_address, TESTER_ADDRESS);
            assert_eq!(activation_response.entity_logical_address, ENTITY_ADDRESS);
            assert_eq!(activation_response.response_code, ROUTING_RESPONSE_SUCCESS);

            // 3. Send UDS ReadDataByIdentifier for VIN (DID 0xF190).
            let uds_request = [0x22, 0xF1, 0x90];

            let diagnostic_request = DiagnosticMessage {
                source_address: TESTER_ADDRESS,
                target_address: ECU_ADDRESS,
                uds_payload: &uds_request,
            }
            .encode();

            client
                .send_frame(0x8001, &diagnostic_request)
                .await
                .expect("failed to send UDS diagnostic request");

            // 4. Verify the response returned by the actual UDS server.
            let diagnostic_frame = timeout(TokioDuration::from_secs(2), client.receive_frame())
                .await
                .expect("VIN diagnostic response timed out")
                .expect("failed to receive VIN diagnostic response");

            assert_eq!(diagnostic_frame.header.payload_type, 0x8001);

            let diagnostic_response = DiagnosticMessage::decode(&diagnostic_frame.payload)
                .expect("invalid diagnostic response");

            assert_eq!(diagnostic_response.source_address, ECU_ADDRESS);
            assert_eq!(diagnostic_response.target_address, TESTER_ADDRESS);

            assert_eq!(
                diagnostic_response.uds_payload,
                b"\x62\xF1\x90SDVDEMO0000000001"
            );
        }
        .await;

        // Shut down the listener and release all dispatcher handles.
        shutdown_tx
            .send(true)
            .expect("failed to send DoIP shutdown signal");

        timeout(Duration::from_secs(3), listener_task)
            .await
            .expect("DoIP listener shutdown timed out")
            .expect("DoIP listener task panicked")
            .expect("DoIP listener returned an error");

        drop(router);
        drop(registry);
        drop(dispatcher);

        actor.join().expect("UDS dispatcher thread panicked");
    }
}
