//! VeloSOVD central SOVD server.
//!
//! Module map (R6):
//!
//! | Module | Responsibility |
//! |---|---|
//! | `main.rs` | startup, configuration, middleware stack, graceful shutdown |
//! | `routes/` | route table (`mod.rs`) and handlers by resource |
//! | `state.rs` | shared `AppState`, ECU address resolution |
//! | `models.rs` | HTTP request/response models |
//! | `api_error.rs` | SOVD GenericError types for handlers |
//! | `zonal_client.rs` | central-to-zonal mTLS client, clear-faults call |
//! | `uds_policy.rs` | raw UDS validation and allow-list (R1) |
//! | `authz.rs` / `limits.rs` / `audit.rs` / `errors.rs` | middleware: R2 / R3 / R4+R10 / R7 |
//! | `execution/` | execution manager, lifecycle model, executor trait |
//! | `api_tests.rs` | HTTP-level integration tests (test builds only) |

mod api_error;
#[cfg(test)]
mod api_tests;
mod audit;
mod authz;
mod errors;
pub mod execution;
mod limits;
mod models;
mod routes;
mod state;
mod tls_config;
mod uds_policy;
mod zonal_client;

use crate::{routes::*, state::*, zonal_client::*};
use axum::{extract::DefaultBodyLimit, middleware};
use std::{sync::Arc, time::Duration};
use tokio::sync::OnceCell;
use vehicle_topology::VehicleTopology;

/// R8: one zonal HTTPS port for all zones (taken from the topology file).
fn uniform_zonal_http_port(topology: &VehicleTopology) -> Result<u16, String> {
    let mut ports: Vec<u16> = topology.zones.iter().map(|z| z.zonal_http_port).collect();
    ports.sort_unstable();
    ports.dedup();
    match ports.as_slice() {
        [port] => Ok(*port),
        [] => Err("topology defines no zones".to_string()),
        many => Err(format!(
            "zones use different zonal_http_port values {many:?}; per-zone ports are not supported yet"
        )),
    }
}

/// R10: resolve on Ctrl-C or SIGTERM (systemd / `kill`).
async fn shutdown_signal() -> &'static str {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => "SIGINT",
        _ = terminate => "SIGTERM",
    }
}

const SHUTDOWN_DRAIN: Duration = Duration::from_secs(10);
const EXECUTION_STOP_GRACE: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() {
    // Offline check:
    //   sovd-server --verify-audit [--allow-partial] <oldest-segment> ... <current-file>
    // Strict by default: the first file must start at genesis (seq 1), so a
    // deleted oldest segment (head truncation) FAILS. Use --allow-partial only
    // when old segments were removed on purpose by the retention policy.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--verify-audit") {
        let allow_partial = args.iter().skip(2).any(|a| a == "--allow-partial");
        let mut files: Vec<String> = args
            .iter()
            .skip(2)
            .filter(|a| a.as_str() != "--allow-partial")
            .cloned()
            .collect();
        if files.is_empty() {
            files.push("audit/central-audit.jsonl".to_string());
        }
        let result = audit::verify_files(&files)
            .and_then(|summary| audit::check_complete(&summary, allow_partial).map(|()| summary));
        match result {
            Ok(summary) => {
                println!(
                    "[AUDIT] OK: {} entries in {} file(s), last seq {}, hash chain intact{}",
                    summary.entries,
                    files.len(),
                    summary.last_seq,
                    if summary.anchored {
                        " (PARTIAL: starts at a segment-start, --allow-partial)"
                    } else {
                        " (complete from genesis)"
                    }
                );
                return;
            }
            Err(reason) => {
                eprintln!("[AUDIT] FAILED: {reason}");
                std::process::exit(2);
            }
        }
    }

    let topology_path = std::env::var("SDV_TOPOLOGY")
        .unwrap_or_else(|_| "platform/config/vehicle-topology.toml".to_string());

    let topology = VehicleTopology::load(&topology_path)
        .unwrap_or_else(|error| panic!("Topology load failed: {error}"));

    let zonal_http_port = uniform_zonal_http_port(&topology)
        .unwrap_or_else(|error| panic!("[SOVD] Topology zonal port error: {error}"));

    let client = build_zonal_mtls_client().unwrap_or_else(|error| {
        panic!("Cannot initialize Central-to-Zonal mTLS client: {error}");
    });

    let state = AppState {
        client,
        topology,
        zonal_http_port,
        // Production path is always HTTPS + mTLS; plain HTTP exists only in tests.
        zonal_scheme: "https",
        execution_manager: Arc::new(OnceCell::new()),
    };

    // R9: build the execution manager now, so a broken setup fails at startup
    // instead of on the first operation request.
    get_execution_manager(&state)
        .await
        .unwrap_or_else(|status| panic!("[SOVD] Execution manager init failed: {status}"));

    let policy_path = std::env::var("SDV_ACCESS_POLICY")
        .unwrap_or_else(|_| "platform/config/access-policy.toml".to_string());
    // M1: user tokens are verified with the auth-service public key.
    let token_key_path = std::env::var("SDV_TOKEN_PUBLIC_KEY").ok();
    let token_verifier = token_key_path.as_ref().map(|path| {
        let pem = std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("[SOVD] Token public key {path}: {error}"));
        sovd_auth::TokenVerifier::from_spki_pem(&pem)
            .unwrap_or_else(|error| panic!("[SOVD] Token public key {path}: {error}"))
    });
    let access_policy = authz::AccessPolicy::load(std::path::Path::new(&policy_path))
        .unwrap_or_else(|error| panic!("[SOVD] Access policy load failed ({policy_path}): {error}"))
        .with_token_verifier(token_verifier);
    let unsatisfiable = access_policy.unsatisfiable_token_principals();
    if !unsatisfiable.is_empty() {
        panic!(
            "[SOVD] Principals {unsatisfiable:?} require user tokens but SDV_TOKEN_PUBLIC_KEY is not set"
        );
    }
    let access_policy = Arc::new(access_policy);
    let audit_path =
        std::env::var("SDV_AUDIT_LOG").unwrap_or_else(|_| "audit/central-audit.jsonl".to_string());
    let audit_max_bytes = std::env::var("SDV_AUDIT_MAX_BYTES")
        .ok()
        .map(|v| {
            v.parse::<u64>()
                .unwrap_or_else(|_| panic!("[SOVD] SDV_AUDIT_MAX_BYTES must be a number: {v}"))
        })
        .unwrap_or(audit::DEFAULT_MAX_BYTES);
    let audit_log = Arc::new(
        audit::AuditLog::open_with_limit(std::path::Path::new(&audit_path), audit_max_bytes)
            .unwrap_or_else(|error| panic!("[SOVD] Audit log open failed ({audit_path}): {error}")),
    );
    let audit_state = audit::AuditState {
        log: Arc::clone(&audit_log),
        policy: Arc::clone(&access_policy),
    };
    let request_limits = Arc::new(limits::RequestLimits::new(
        limits::DEFAULT_MAX_CONCURRENT_REQUESTS,
        limits::DEFAULT_REQUEST_TIMEOUT,
    ));

    // Layer order: last added runs first.
    // json errors -> limits (503/504) -> audit (records all below) -> authz (403) -> body limit (413) -> routes
    let app = build_router(state.clone())
        .layer(DefaultBodyLimit::max(limits::DEFAULT_MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(
            access_policy,
            authz::authorize,
        ))
        .layer(middleware::from_fn_with_state(audit_state, audit::audit))
        .layer(middleware::from_fn_with_state(
            request_limits,
            limits::enforce,
        ))
        .layer(middleware::from_fn(errors::json_errors));

    let bind_address = format!(
        "{}:{}",
        state.topology.central.sovd_bind, state.topology.central.sovd_port
    );

    let client_ca_path = std::env::var("SDV_TLS_CLIENT_CA_CERT")
        .unwrap_or_else(|_| "pki/dev/client-ca/ca.crt".to_string());

    let server_cert_path = std::env::var("SDV_TLS_SERVER_CERT")
        .unwrap_or_else(|_| "pki/dev/central/central-server.crt".to_string());

    let server_key_path = std::env::var("SDV_TLS_SERVER_KEY")
        .unwrap_or_else(|_| "pki/dev/central/central-server.key".to_string());

    let tls_config = tls_config::load_server_config(
        std::path::Path::new(&client_ca_path),
        std::path::Path::new(&server_cert_path),
        std::path::Path::new(&server_key_path),
    )
    .unwrap_or_else(|error| {
        panic!("[SOVD] Central HTTPS/mTLS configuration failed: {error}");
    });

    let listener = tokio::net::TcpListener::bind(&bind_address)
        .await
        .unwrap_or_else(|error| {
            panic!("Cannot bind central SOVD server at {bind_address}: {error}")
        });

    let std_listener = listener
        .into_std()
        .expect("Cannot convert Central TCP listener");

    let tls_server = axum_server::from_tcp_rustls(std_listener, tls_config)
        .expect("Cannot initialize Central TLS server");

    println!(
        "[SOVD] Vehicle: {} ({})",
        state.topology.vehicle.name, state.topology.vehicle.id
    );
    println!("[SOVD] Listening at https://{bind_address} (TLS 1.3, mTLS required)");
    println!("[SOVD] Configured zones: {}", state.topology.zones.len());
    println!("[SOVD] Configured ECUs: {}", state.topology.ecus.len());
    println!("[SOVD] Zonal endpoint: https://<zonal_ip>:{zonal_http_port} (from topology)");
    println!(
        "[SOVD] Operation history capacity: {}",
        EXECUTION_HISTORY_CAPACITY
    );
    println!("[SOVD] Execution manager: ready");
    println!("[SOVD] Access policy: {policy_path} (deny by default)");
    match &token_key_path {
        Some(path) => println!("[SOVD] User tokens: accepted (key {path})"),
        None => println!("[SOVD] User tokens: not accepted (SDV_TOKEN_PUBLIC_KEY unset)"),
    }
    println!(
        "[SOVD] Audit log: {audit_path} (SHA-256 hash chain, rotate at {audit_max_bytes} bytes)"
    );
    println!(
        "[SOVD] Limits: {} concurrent, {:?} timeout, {} byte body",
        limits::DEFAULT_MAX_CONCURRENT_REQUESTS,
        limits::DEFAULT_REQUEST_TIMEOUT,
        limits::DEFAULT_MAX_BODY_BYTES
    );

    if let Err(error) = audit_log.record_event("server-start", &bind_address) {
        eprintln!("[AUDIT] write failed: {error}");
    }

    // R10: graceful shutdown. Stop accepting, drain in-flight requests,
    // then stop executions and close the audit chain.
    let handle = axum_server::Handle::new();
    let signal_handle = handle.clone();
    let (signal_tx, signal_rx) = tokio::sync::oneshot::channel::<&'static str>();
    tokio::spawn(async move {
        let signal = shutdown_signal().await;
        println!("[SOVD] {signal} received; draining requests for up to {SHUTDOWN_DRAIN:?}");
        let _ = signal_tx.send(signal);
        signal_handle.graceful_shutdown(Some(SHUTDOWN_DRAIN));
    });

    tls_server
        .handle(handle)
        .serve(app.into_make_service())
        .await
        .expect("Central HTTPS server failed");

    let signal = signal_rx.await.unwrap_or("server-exit");
    let mut detail = format!("signal={signal}");
    if let Some(manager) = state.execution_manager.get() {
        let unfinished = manager.shutdown(EXECUTION_STOP_GRACE).await;
        if !unfinished.is_empty() {
            println!("[SOVD] Executions not stopped cleanly: {unfinished:?}");
        }
        detail.push_str(&format!(" unfinished_executions={}", unfinished.len()));
    }
    if let Err(error) = audit_log.record_event("server-stop", &detail) {
        eprintln!("[AUDIT] write failed: {error}");
    }
    println!("[SOVD] Shutdown complete ({detail})");
}

#[cfg(test)]
mod zonal_port_tests {
    use super::uniform_zonal_http_port;
    use vehicle_topology::VehicleTopology;

    fn topology() -> VehicleTopology {
        VehicleTopology::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../platform/config/vehicle-topology.toml"
        ))
        .expect("Test topology must load")
    }

    #[test]
    fn uses_the_configured_port() {
        let mut t = topology();
        for zone in &mut t.zones {
            zone.zonal_http_port = 9443;
        }
        assert_eq!(uniform_zonal_http_port(&t), Ok(9443));
    }

    #[test]
    fn rejects_mixed_ports_and_no_zones() {
        let mut t = topology();
        t.zones[0].zonal_http_port = 9443;
        assert!(
            uniform_zonal_http_port(&t)
                .unwrap_err()
                .contains("different")
        );
        t.zones.clear();
        assert!(uniform_zonal_http_port(&t).is_err());
    }
}
