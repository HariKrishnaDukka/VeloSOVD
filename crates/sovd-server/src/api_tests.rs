//! HTTP-level integration tests of the central router (mock zonal services).

use crate::execution::model::ExecutionState;
use crate::routes::*;
use crate::{models::*, state::*};
use axum::{
    Json, Router,
    extract::Path,
    http::{StatusCode, header},
};
use diagnostic_common::DiagnosticFault;
use reqwest::Client;
use std::{sync::Arc, time::Duration};
use tokio::sync::OnceCell;
use vehicle_topology::VehicleTopology;

#[cfg(test)]
mod payload_forwarding_integration_tests {
    use super::*;
    use axum::{extract::State, routing::post};
    use serde_json::Value;
    use std::{
        path::PathBuf,
        sync::{
            Arc, Mutex as StdMutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    type MockEcuState = (Arc<AtomicUsize>, Arc<StdMutex<Option<String>>>);

    async fn mock_ecu(
        State((request_count, received_payload)): State<MockEcuState>,
        Json(request): Json<UdsDiagnosticRequest>,
    ) -> Json<UdsDiagnosticResponse> {
        request_count.fetch_add(1, Ordering::SeqCst);

        let forwarded_payload = request.payload_hex;
        *received_payload
            .lock()
            .expect("Payload capture mutex must not be poisoned") = Some(forwarded_payload.clone());

        Json(UdsDiagnosticResponse {
            ecu_id: "ECM".to_string(),
            request_hex: forwarded_payload,
            response_hex: Some("62F190".to_string()),
        })
    }

    #[tokio::test]
    async fn operation_execution_routes_filter_by_operation_and_ecu() {
        let topology_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../platform/config/vehicle-topology.toml");
        let topology = VehicleTopology::load(&topology_path).expect("Test topology must load");

        let state = AppState {
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("HTTP client must initialize"),
            topology,
            zonal_http_port: 9001,
            zonal_scheme: "http",
            execution_manager: Arc::new(OnceCell::new()),
        };

        // R5: seed the single ExecutionManager store (no legacy VecDeque).
        let manager = get_execution_manager(&state)
            .await
            .expect("Execution manager must initialize");
        let mut seeded = Vec::new();
        for (ecu, fail) in [("ECM", false), ("ECM", true), ("BCM", false)] {
            let record = manager
                .create_execution(ecu.to_string(), "clear-faults", 1000)
                .await
                .expect("Seed execution must be created");
            manager
                .transition_execution(&record.execution_id, ExecutionState::Running, 1001)
                .await
                .expect("Seed execution must start");
            let end = if fail {
                ExecutionState::Failed
            } else {
                ExecutionState::Completed
            };
            manager
                .transition_execution(&record.execution_id, end, 1002)
                .await
                .expect("Seed execution must finish");
            seeded.push(record.execution_id);
        }
        let (first_id, failed_id) = (seeded[0].clone(), seeded[1].clone());

        let app = build_router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Central test listener must bind");
        let address = listener
            .local_addr()
            .expect("Central test address must be available");

        let server_task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("Central test server failed");
        });

        let client = Client::new();
        let base_url = format!("http://{address}");

        // Only ECM executions belonging to clear-faults should be listed.
        let response = client
            .get(format!(
                "{base_url}/api/ecus/ECM/operations/clear-faults/executions"
            ))
            .send()
            .await
            .expect("Execution list request must complete");

        assert_eq!(response.status(), StatusCode::OK);
        let listed: serde_json::Value = response
            .json()
            .await
            .expect("Execution list response must be valid JSON");

        let listed_items = listed
            .as_array()
            .expect("Execution list must be a JSON array");

        assert_eq!(listed_items.len(), 2);
        assert_eq!(listed_items[0]["execution_id"], first_id.as_str());
        assert_eq!(listed_items[0]["operation"], "clear-faults");
        assert_eq!(listed_items[0]["ecu_id"], "ECM");
        assert_eq!(listed_items[1]["execution_id"], failed_id.as_str());
        assert_eq!(listed_items[1]["success"], false);

        // The same execution must be retrievable under its owning operation.
        let response = client
            .get(format!(
                "{base_url}/api/ecus/ECM/operations/clear-faults/executions/{first_id}"
            ))
            .send()
            .await
            .expect("Execution detail request must complete");

        assert_eq!(response.status(), StatusCode::OK);
        let detail: serde_json::Value = response
            .json()
            .await
            .expect("Execution detail response must be valid JSON");

        assert_eq!(detail["execution_id"], first_id.as_str());
        assert_eq!(detail["operation"], "clear-faults");

        // A different operation ID must not expose the same execution.
        let response = client
            .get(format!(
                "{base_url}/api/ecus/ECM/operations/read-status/executions/{first_id}"
            ))
            .send()
            .await
            .expect("Mismatched operation request must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        server_task.abort();
    }

    /// SIL rule: central has no route that creates or injects faults.
    #[tokio::test]
    async fn faults_collection_rejects_post_and_put() {
        let topology_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../platform/config/vehicle-topology.toml");
        let topology = VehicleTopology::load(&topology_path).expect("Test topology must load");

        let state = AppState {
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("HTTP client must initialize"),
            topology,
            zonal_http_port: 9001,
            zonal_scheme: "http",
            execution_manager: Arc::new(OnceCell::new()),
        };

        let app = build_router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Central test listener must bind");
        let address = listener
            .local_addr()
            .expect("Central test address must be available");
        let central_task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("Central test server failed");
        });

        let client = Client::new();

        // The fault routes only know GET and DELETE: any write that could create a fault is 405.
        for path in [
            "/api/ecus/ECM/diagnostics/faults",
            "/api/ecus/ECM/diagnostics/faults/P0101",
        ] {
            for method in [
                reqwest::Method::POST,
                reqwest::Method::PUT,
                reqwest::Method::PATCH,
            ] {
                let response = client
                    .request(method.clone(), format!("http://{address}{path}"))
                    .send()
                    .await
                    .expect("Fault write request must complete");
                assert_eq!(
                    response.status(),
                    StatusCode::METHOD_NOT_ALLOWED,
                    "{method} {path}"
                );
            }
        }

        // No injection-style route exists under any obvious name.
        for path in [
            "/api/ecus/ECM/diagnostics/inject",
            "/api/ecus/ECM/diagnostics/faults/inject",
            "/api/ecus/ECM/faults",
            "/api/faults",
        ] {
            let response = client
                .post(format!("http://{address}{path}"))
                .send()
                .await
                .expect("Injection probe must complete");
            assert!(!response.status().is_success(), "POST {path}");
        }

        central_task.abort();
    }

    /// The operation list and the single description publish what the executor implements.
    #[tokio::test]
    async fn operation_list_publishes_capabilities() {
        let topology_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../platform/config/vehicle-topology.toml");
        let topology = VehicleTopology::load(&topology_path).expect("Test topology must load");

        let state = AppState {
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("HTTP client must initialize"),
            topology,
            zonal_http_port: 9001,
            zonal_scheme: "http",
            execution_manager: Arc::new(OnceCell::new()),
        };

        let app = build_router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Central test listener must bind");
        let address = listener
            .local_addr()
            .expect("Central test address must be available");
        let central_task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("Central test server failed");
        });

        let client = Client::new();

        let response = client
            .get(format!("http://{address}/api/ecus/ECM/operations"))
            .send()
            .await
            .expect("Operation list request must complete");
        assert_eq!(response.status(), StatusCode::OK);
        let list: Value = response
            .json()
            .await
            .expect("Operation list must be valid JSON");
        let operations = list.as_array().expect("Operation list must be an array");
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0]["id"], "clear-faults");

        // clear-faults can be started and polled but not stopped, frozen or reset.
        let capabilities = &operations[0]["capabilities"];
        assert_eq!(capabilities["execute"], true);
        assert_eq!(capabilities["status"], true);
        assert_eq!(capabilities["stop"], false);
        assert_eq!(capabilities["freeze"], false);
        assert_eq!(capabilities["reset"], false);

        let response = client
            .get(format!(
                "http://{address}/api/ecus/ECM/operation-descriptions/clear-faults"
            ))
            .send()
            .await
            .expect("Operation description request must complete");
        assert_eq!(response.status(), StatusCode::OK);
        let one: Value = response
            .json()
            .await
            .expect("Operation description must be valid JSON");
        assert_eq!(one["capabilities"], operations[0]["capabilities"]);

        central_task.abort();
    }

    #[tokio::test]
    async fn retrieves_individual_operation_description() {
        let topology_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../platform/config/vehicle-topology.toml");

        let topology = VehicleTopology::load(&topology_path).expect("Test topology must load");

        let state = AppState {
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("HTTP client must initialize"),
            topology,
            zonal_http_port: 9001,
            zonal_scheme: "http",
            execution_manager: Arc::new(OnceCell::new()),
        };

        let app = build_router(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Central test listener must bind");

        let address = listener
            .local_addr()
            .expect("Central test address must be available");

        let central_task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("Central test server failed");
        });

        let client = Client::new();

        // Test 1: Retrieve the existing clear-faults operation.
        let response = client
            .get(format!(
                "http://{address}/api/ecus/ECM/operation-descriptions/clear-faults"
            ))
            .send()
            .await
            .expect("Operation description request must complete");

        assert_eq!(response.status(), StatusCode::OK);

        let body: Value = response
            .json()
            .await
            .expect("Operation description must be valid JSON");

        assert_eq!(body["id"], "clear-faults");
        assert_eq!(body["name"], "Clear fault memory");
        assert_eq!(body["method"], "POST");
        assert_eq!(body["supports_execution"], true);
        assert_eq!(
            body["execution_path"],
            "/api/ecus/ECM/operations/clear-faults/executions"
        );

        // Test 2: An unknown operation must return 404.
        let response = client
            .get(format!(
                "http://{address}/api/ecus/ECM/operation-descriptions/unknown-operation"
            ))
            .send()
            .await
            .expect("Unknown operation request must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // Test 3: An unknown ECU must return 404.
        let response = client
            .get(format!(
                "http://{address}/api/ecus/UNKNOWN/operation-descriptions/clear-faults"
            ))
            .send()
            .await
            .expect("Unknown ECU request must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        central_task.abort();
    }

    #[tokio::test]
    async fn forwards_normalized_payload_and_rejects_invalid_requests() {
        // Load a private in-memory copy of the project topology.
        let topology_path: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../platform/config/vehicle-topology.toml");

        let mut topology = VehicleTopology::load(&topology_path).expect("Test topology must load");

        // Redirect FRONT zone to the local mock ECU.
        let front_zone = topology
            .zones
            .iter_mut()
            .find(|zone| zone.id == "FRONT")
            .expect("FRONT zone must exist");

        front_zone.zonal_ip = "127.0.0.1".to_string();

        // Start mock ECU on an operating-system-assigned port.
        let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Mock ECU listener must bind");

        let mock_port = mock_listener
            .local_addr()
            .expect("Mock ECU address must be available")
            .port();

        let request_count = Arc::new(AtomicUsize::new(0));
        let received_payload = Arc::new(StdMutex::new(None));

        let mock_app = Router::new()
            .route("/ecus/ECM/diagnostics/requests", post(mock_ecu))
            .with_state((Arc::clone(&request_count), Arc::clone(&received_payload)));

        let mock_task = tokio::spawn(async move {
            axum::serve(mock_listener, mock_app)
                .await
                .expect("Mock ECU server failed");
        });

        // Construct the actual central SOVD application.
        let state = AppState {
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("HTTP client must initialize"),
            topology,
            zonal_http_port: mock_port,
            zonal_scheme: "http",
            execution_manager: Arc::new(OnceCell::new()),
        };

        let app = build_router(state);

        let central_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Central test listener must bind");

        let central_address = central_listener
            .local_addr()
            .expect("Central test address must be available");

        let central_task = tokio::spawn(async move {
            axum::serve(central_listener, app)
                .await
                .expect("Central test server failed");
        });

        let client = Client::new();
        let endpoint = format!(
            "http://{}/api/ecus/ECM/diagnostics/requests",
            central_address
        );

        // Test 1: Whitespace is removed before forwarding.
        let response = client
            .post(&endpoint)
            .json(&serde_json::json!({
                "payload_hex": "22 f1 90"
            }))
            .send()
            .await
            .expect("Central diagnostic request must complete");

        assert_eq!(response.status(), StatusCode::OK);

        let response_body: Value = response
            .json()
            .await
            .expect("Central response must be valid JSON");

        assert_eq!(response_body["ecu_id"], "ECM");
        assert_eq!(response_body["request_hex"], "22f190");
        assert_eq!(response_body["response_hex"], "62F190");

        assert_eq!(
            request_count.load(Ordering::SeqCst),
            1,
            "Mock ECU must receive exactly one valid request"
        );

        assert_eq!(
            received_payload
                .lock()
                .expect("Payload capture mutex must not be poisoned")
                .as_deref(),
            Some("22f190"),
            "Mock ECU must receive the normalized hexadecimal payload"
        );

        // Test 2: Invalid hexadecimal must be rejected before forwarding.
        let response = client
            .post(&endpoint)
            .json(&serde_json::json!({
                "payload_hex": "22G190"
            }))
            .send()
            .await
            .expect("Invalid diagnostic request must complete");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        assert_eq!(
            request_count.load(Ordering::SeqCst),
            1,
            "Invalid payload must not reach the mock ECU"
        );

        // Test 3: Unknown ECU must be rejected before forwarding.
        let unknown_endpoint = format!(
            "http://{}/api/ecus/UNKNOWN/diagnostics/requests",
            central_address
        );

        let response = client
            .post(&unknown_endpoint)
            .json(&serde_json::json!({
                "payload_hex": "22F190"
            }))
            .send()
            .await
            .expect("Unknown ECU request must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        assert_eq!(
            request_count.load(Ordering::SeqCst),
            1,
            "Unknown ECU request must not reach the mock ECU"
        );

        central_task.abort();
        mock_task.abort();
    }

    #[tokio::test]
    async fn sovd_post_executes_operation_and_records_execution() {
        use axum::routing::post;

        type MockDtcStore = Arc<StdMutex<Vec<String>>>;

        async fn mock_clear_faults(
            State(dtcs): State<MockDtcStore>,
        ) -> Json<UdsDiagnosticResponse> {
            dtcs.lock()
                .expect("Mock DTC store lock must succeed")
                .clear();

            Json(UdsDiagnosticResponse {
                ecu_id: "ECM".to_string(),
                request_hex: "14FFFFFF".to_string(),
                response_hex: Some("54".to_string()),
            })
        }

        let topology_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../platform/config/vehicle-topology.toml");

        let mut topology = VehicleTopology::load(&topology_path).expect("Test topology must load");

        for zone in &mut topology.zones {
            zone.zonal_ip = "127.0.0.1".to_string();
        }

        let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Mock zonal listener must bind");

        let mock_port = mock_listener
            .local_addr()
            .expect("Mock zonal address must be available")
            .port();

        let seeded_dtcs = Arc::new(StdMutex::new(vec!["P0128".to_string()]));
        let mock_app = Router::new()
            .route(
                "/ecus/ECM/diagnostics/operations/clear-faults",
                post(mock_clear_faults),
            )
            .with_state(Arc::clone(&seeded_dtcs));

        let mock_task = tokio::spawn(async move {
            axum::serve(mock_listener, mock_app)
                .await
                .expect("Mock zonal server failed");
        });

        let state = AppState {
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("HTTP client must initialize"),
            topology,
            zonal_http_port: mock_port,
            zonal_scheme: "http",
            execution_manager: Arc::new(OnceCell::new()),
        };

        let app = build_router(state);

        let central_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Central listener must bind");

        let central_address = central_listener
            .local_addr()
            .expect("Central address must be available");

        let central_task = tokio::spawn(async move {
            axum::serve(central_listener, app)
                .await
                .expect("Central server failed");
        });

        let client = Client::new();
        let base_url = format!("http://{central_address}");

        // Execute clear-faults through the SOVD-facing endpoint.
        let response = client
            .post(format!(
                "{base_url}/sovd/v1/components/ECM/operations/clear-faults/executions"
            ))
            .json(&serde_json::json!({}))
            .send()
            .await
            .expect("SOVD execution request must complete");

        assert_eq!(response.status(), StatusCode::ACCEPTED);

        let response_location = response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);

        let start_result: Value = response
            .json()
            .await
            .expect("SOVD asynchronous response must be valid JSON");

        let execution_id = start_result["id"]
            .as_str()
            .expect("Asynchronous response must contain an execution ID")
            .to_string();

        assert_eq!(start_result["status"], "running");

        let expected_location =
            format!("/sovd/v1/components/ECM/operations/clear-faults/executions/{execution_id}");
        assert_eq!(
            response_location.as_deref(),
            Some(expected_location.as_str())
        );

        // Verify that the execution was stored under the route operation ID.
        let response = client
            .get(format!(
                "{base_url}/sovd/v1/components/ECM/operations/clear-faults/executions"
            ))
            .send()
            .await
            .expect("SOVD execution collection request must complete");

        assert_eq!(response.status(), StatusCode::OK);

        let collection: Value = response
            .json()
            .await
            .expect("SOVD collection must be valid JSON");

        let items = collection["items"]
            .as_array()
            .expect("SOVD collection items must be an array");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].as_str(), Some(execution_id.as_str()));
        assert!(execution_id.starts_with("exec-"));

        // Poll the execution resource because the zonal operation runs
        // asynchronously. Do not assume completion immediately after POST.
        let mut final_status = Value::Null;
        let mut reached_terminal_state = false;

        for _ in 0..100 {
            let response = client
                .get(format!(
                    "{base_url}/sovd/v1/components/ECM/operations/clear-faults/executions/{execution_id}"
                ))
                .send()
                .await
                .expect("SOVD execution status request must complete");

            assert_eq!(response.status(), StatusCode::OK);

            final_status = response
                .json()
                .await
                .expect("SOVD execution status must be valid JSON");

            let status = final_status["status"].as_str().unwrap_or_default();
            if status == "completed" || status == "failed" {
                reached_terminal_state = true;
                break;
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert!(
            reached_terminal_state,
            "Execution did not reach a terminal state: {final_status}"
        );
        assert_eq!(final_status["status"], "completed");
        assert_eq!(final_status["capability"], "execute");
        assert!(final_status.get("error").is_none());
        assert!(
            seeded_dtcs.lock().unwrap().is_empty(),
            "The seeded DTC must be cleared by the accepted UDS positive response"
        );

        // Unsupported operation IDs must not be forwarded.
        let response = client
            .post(format!(
                "{base_url}/sovd/v1/components/ECM/operations/unknown-operation/executions"
            ))
            .json(&serde_json::json!({}))
            .send()
            .await
            .expect("Unknown operation request must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // Unsupported entity collections must be rejected.
        let response = client
            .post(format!(
                "{base_url}/sovd/v1/unknown/ECM/operations/clear-faults/executions"
            ))
            .json(&serde_json::json!({}))
            .send()
            .await
            .expect("Unknown entity collection request must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        central_task.abort();
        mock_task.abort();
    }

    #[tokio::test]
    async fn list_ecus_uses_configured_zonal_http_port() {
        use axum::routing::get;

        async fn mock_ecus() -> Json<Vec<diagnostic_common::EcuInfo>> {
            Json(vec![diagnostic_common::EcuInfo {
                id: "ECM".to_string(),
                name: "Engine Control Module".to_string(),
                domain: "powertrain".to_string(),
                status: "online".to_string(),
            }])
        }

        let topology_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../platform/config/vehicle-topology.toml");

        let mut topology = VehicleTopology::load(&topology_path).expect("Test topology must load");

        // let front_zone = topology
        //     .zones
        //     .iter_mut()
        //     .find(|zone| zone.id == "FRONT")
        //     .expect("FRONT zone must exist");

        for zone in &mut topology.zones {
            zone.zonal_ip = "127.0.0.1".to_string();
        }

        let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Mock listener must bind");

        let mock_port = mock_listener.local_addr().unwrap().port();

        let mock_app = Router::new().route("/ecus", get(mock_ecus));

        let mock_task = tokio::spawn(async move {
            axum::serve(mock_listener, mock_app)
                .await
                .expect("Mock server failed");
        });

        let state = AppState {
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("HTTP client must initialize"),
            topology,
            zonal_http_port: mock_port,
            zonal_scheme: "http",
            execution_manager: Arc::new(OnceCell::new()),
        };

        let app = build_router(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Central listener must bind");

        let address = listener.local_addr().unwrap();

        let central_task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("Central server failed");
        });

        let response = Client::new()
            .get(format!("http://{address}/api/ecus"))
            .send()
            .await
            .expect("ECU discovery request must complete");

        assert_eq!(response.status(), StatusCode::OK);

        let ecus: Vec<diagnostic_common::EcuInfo> = response
            .json()
            .await
            .expect("ECU discovery response must be valid JSON");

        assert!(
            ecus.iter().any(|ecu| ecu.id == "ECM"),
            "ECU discovery must receive the mock ECU through the configured port"
        );

        central_task.abort();
        mock_task.abort();
    }

    #[tokio::test]
    async fn health_uses_configured_zonal_http_port() {
        use axum::routing::get;

        async fn mock_ecus() -> Json<Vec<diagnostic_common::EcuInfo>> {
            Json(Vec::new())
        }

        let topology_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../platform/config/vehicle-topology.toml");

        let mut topology = VehicleTopology::load(&topology_path).expect("Test topology must load");

        let front_zone = topology
            .zones
            .iter_mut()
            .find(|zone| zone.id == "FRONT")
            .expect("FRONT zone must exist");

        front_zone.zonal_ip = "127.0.0.1".to_string();

        let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Mock listener must bind");

        let mock_port = mock_listener.local_addr().unwrap().port();

        let mock_app = Router::new().route("/ecus", get(mock_ecus));

        let mock_task = tokio::spawn(async move {
            axum::serve(mock_listener, mock_app)
                .await
                .expect("Mock server failed");
        });

        let state = AppState {
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("HTTP client must initialize"),
            topology,
            zonal_http_port: mock_port,
            zonal_scheme: "http",
            execution_manager: Arc::new(OnceCell::new()),
        };

        let app = build_router(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Central listener must bind");

        let address = listener.local_addr().unwrap();

        let central_task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("Central server failed");
        });

        let response = Client::new()
            .get(format!("http://{address}/api/health"))
            .send()
            .await
            .expect("Health request must complete");

        assert_eq!(response.status(), StatusCode::OK);

        let body: Value = response
            .json()
            .await
            .expect("Health response must be valid JSON");

        assert!(
            body.to_string().contains("online"),
            "Health must report the reachable FRONT zone as online"
        );

        central_task.abort();
        mock_task.abort();
    }

    #[tokio::test]
    async fn retrieves_individual_fault_from_zonal_server() {
        use axum::routing::get;

        async fn mock_faults(
            Path(ecu_id): Path<String>,
        ) -> Result<Json<Vec<DiagnosticFault>>, StatusCode> {
            if ecu_id != "ECM" {
                return Err(StatusCode::NOT_FOUND);
            }

            Ok(Json(vec![
                DiagnosticFault {
                    code: "P0128".to_string(),
                    description: "Coolant thermostat temperature below threshold".to_string(),
                    active: true,
                    status: 0x09,
                },
                DiagnosticFault {
                    code: "P0300".to_string(),
                    description: "Random or multiple cylinder misfire detected".to_string(),
                    active: false,
                    status: 0x08,
                },
            ]))
        }
        async fn mock_fault(
            Path((ecu_id, fault_code)): Path<(String, String)>,
        ) -> Result<Json<DiagnosticFault>, StatusCode> {
            if ecu_id != "ECM" || fault_code != "P0128" {
                return Err(StatusCode::NOT_FOUND);
            }

            Ok(Json(DiagnosticFault {
                code: "P0128".to_string(),
                description: "Coolant thermostat temperature below threshold".to_string(),
                active: true,
                status: 0x09,
            }))
        }

        async fn mock_delete_faults(Path(ecu_id): Path<String>) -> Result<StatusCode, StatusCode> {
            if ecu_id != "ECM" {
                return Err(StatusCode::NOT_FOUND);
            }

            Ok(StatusCode::NO_CONTENT)
        }

        async fn mock_delete_fault(
            Path((ecu_id, fault_code)): Path<(String, String)>,
        ) -> Result<StatusCode, StatusCode> {
            if ecu_id != "ECM" || fault_code != "P0128" {
                return Err(StatusCode::NOT_FOUND);
            }

            Ok(StatusCode::NO_CONTENT)
        }

        let topology_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../platform/config/vehicle-topology.toml");

        let mut topology = VehicleTopology::load(&topology_path).expect("Test topology must load");

        for zone in &mut topology.zones {
            zone.zonal_ip = "127.0.0.1".to_string();
        }

        let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Mock listener must bind");

        let mock_port = mock_listener
            .local_addr()
            .expect("Mock address must be available")
            .port();

        let mock_app = Router::new()
            .route(
                "/ecus/{id}/diagnostics/faults",
                get(mock_faults).delete(mock_delete_faults),
            )
            .route(
                "/ecus/{id}/diagnostics/faults/{fault_code}",
                get(mock_fault).delete(mock_delete_fault),
            );

        let mock_task = tokio::spawn(async move {
            axum::serve(mock_listener, mock_app)
                .await
                .expect("Mock server failed");
        });

        let state = AppState {
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("HTTP client must initialize"),
            topology,
            zonal_http_port: mock_port,
            zonal_scheme: "http",
            execution_manager: Arc::new(OnceCell::new()),
        };

        let app = build_router(state);

        let central_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Central listener must bind");

        let central_address = central_listener
            .local_addr()
            .expect("Central address must be available");

        let central_task = tokio::spawn(async move {
            axum::serve(central_listener, app)
                .await
                .expect("Central server failed");
        });

        let client = Client::new();

        // Test 1: Existing fault must be returned successfully.
        let endpoint = format!(
            "http://{}/api/ecus/ECM/diagnostics/faults/P0128",
            central_address
        );

        let response = client
            .get(&endpoint)
            .send()
            .await
            .expect("Fault request must complete");

        assert_eq!(response.status(), StatusCode::OK);

        let fault: DiagnosticFault = response
            .json()
            .await
            .expect("Fault response must be valid JSON");

        assert_eq!(fault.code, "P0128");
        assert_eq!(
            fault.description,
            "Coolant thermostat temperature below threshold"
        );
        assert!(fault.active);

        // Test 2: No filter must return both faults.
        let fault_list_endpoint =
            format!("http://{}/api/ecus/ECM/diagnostics/faults", central_address);

        let response = client
            .get(&fault_list_endpoint)
            .send()
            .await
            .expect("Fault-list request must complete");

        assert_eq!(response.status(), StatusCode::OK);

        let faults: Vec<DiagnosticFault> = response
            .json()
            .await
            .expect("Fault-list response must be valid JSON");

        assert_eq!(faults.len(), 2);

        // Test 3: active=true must return only active faults.
        let response = client
            .get(format!("{fault_list_endpoint}?active=true"))
            .send()
            .await
            .expect("Active fault filter request must complete");

        assert_eq!(response.status(), StatusCode::OK);

        let faults: Vec<DiagnosticFault> = response
            .json()
            .await
            .expect("Active fault response must be valid JSON");

        assert_eq!(faults.len(), 1);
        assert_eq!(faults[0].code, "P0128");
        assert!(faults[0].active);

        // Test 4: active=false must return only inactive faults.
        let response = client
            .get(format!("{fault_list_endpoint}?active=false"))
            .send()
            .await
            .expect("Inactive fault filter request must complete");

        assert_eq!(response.status(), StatusCode::OK);

        let faults: Vec<DiagnosticFault> = response
            .json()
            .await
            .expect("Inactive fault response must be valid JSON");

        assert_eq!(faults.len(), 1);
        assert_eq!(faults[0].code, "P0300");
        assert!(!faults[0].active);

        // Test 2: Unknown fault must return 404.
        let unknown_fault_endpoint = format!(
            "http://{}/api/ecus/ECM/diagnostics/faults/P9999",
            central_address
        );

        let response = client
            .get(&unknown_fault_endpoint)
            .send()
            .await
            .expect("Unknown fault request must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // Test 3: Unknown ECU must return 404.
        let unknown_ecu_endpoint = format!(
            "http://{}/api/ecus/UNKNOWN/diagnostics/faults/P0128",
            central_address
        );

        let response = client
            .get(&unknown_ecu_endpoint)
            .send()
            .await
            .expect("Unknown ECU request must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // Test 4: Delete an existing individual fault.
        let delete_fault_endpoint = format!(
            "http://{}/api/ecus/ECM/diagnostics/faults/P0128",
            central_address
        );

        let response = client
            .delete(&delete_fault_endpoint)
            .send()
            .await
            .expect("Individual fault deletion must complete");

        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Test 5: Deleting an unknown fault must return 404.
        let unknown_delete_endpoint = format!(
            "http://{}/api/ecus/ECM/diagnostics/faults/P9999",
            central_address
        );

        let response = client
            .delete(&unknown_delete_endpoint)
            .send()
            .await
            .expect("Unknown fault deletion must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // Test 6: Delete the complete fault collection.
        let delete_collection_endpoint =
            format!("http://{}/api/ecus/ECM/diagnostics/faults", central_address);

        let response = client
            .delete(&delete_collection_endpoint)
            .send()
            .await
            .expect("Fault collection deletion must complete");

        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Test 7: Unknown ECU collection deletion must return 404.
        let unknown_collection_endpoint = format!(
            "http://{}/api/ecus/UNKNOWN/diagnostics/faults",
            central_address
        );

        let response = client
            .delete(&unknown_collection_endpoint)
            .send()
            .await
            .expect("Unknown ECU deletion must complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        central_task.abort();
        mock_task.abort();
    }
}

#[cfg(test)]
mod clear_fault_history_tests {
    use super::*;
    use axum::routing::post;
    use serde_json::Value;
    use std::path::PathBuf;

    async fn mock_negative_clear() -> Json<UdsDiagnosticResponse> {
        Json(UdsDiagnosticResponse {
            ecu_id: "ECM".to_string(),
            request_hex: "14FFFFFF".to_string(),
            response_hex: Some("7F1431".to_string()),
        })
    }

    #[tokio::test]
    async fn negative_uds_response_is_recorded_as_failed_execution() {
        let topology_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../platform/config/vehicle-topology.toml");
        let mut topology = VehicleTopology::load(&topology_path).expect("Test topology must load");

        for zone in &mut topology.zones {
            zone.zonal_ip = "127.0.0.1".to_string();
        }

        let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Mock zonal listener must bind");
        let mock_port = mock_listener.local_addr().unwrap().port();
        let mock_app = Router::new().route(
            "/ecus/ECM/diagnostics/operations/clear-faults",
            post(mock_negative_clear),
        );
        let mock_task = tokio::spawn(async move {
            axum::serve(mock_listener, mock_app)
                .await
                .expect("Mock zonal server failed");
        });

        let state = AppState {
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("HTTP client must initialize"),
            topology,
            zonal_http_port: mock_port,
            zonal_scheme: "http",
            execution_manager: Arc::new(OnceCell::new()),
        };
        let app = build_router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Central listener must bind");
        let address = listener.local_addr().unwrap();
        let central_task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("Central server failed");
        });

        let client = Client::new();
        let base = format!("http://{address}");
        let response = client
            .post(format!(
                "{base}/api/ecus/ECM/operations/clear-faults/executions"
            ))
            .send()
            .await
            .expect("Clear execution request must complete");
        assert_eq!(response.status(), StatusCode::OK);

        let execution: Value = response.json().await.unwrap();
        assert_eq!(execution["success"], false);
        assert_eq!(execution["status"], "failed");
        assert!(execution["message"].as_str().unwrap().contains("NRC 0x31"));
        assert!(!execution["message"].as_str().unwrap().starts_with('{'));

        let history_response = client
            .get(format!(
                "{base}/api/ecus/ECM/operations/clear-faults/executions"
            ))
            .send()
            .await
            .expect("Execution history request must complete");
        assert_eq!(history_response.status(), StatusCode::OK);
        let history: Value = history_response.json().await.unwrap();
        assert_eq!(history.as_array().unwrap().len(), 1);
        assert_eq!(history[0]["success"], false);
        assert_eq!(history[0]["status"], "failed");

        // The SOVD-facing execution adapter must expose the same failure as
        // a generic error object with a kebab-case error_code.
        let start = client
            .post(format!(
                "{base}/sovd/v1/components/ECM/operations/clear-faults/executions"
            ))
            .json(&serde_json::json!({}))
            .send()
            .await
            .expect("SOVD negative-response execution must start");
        assert_eq!(start.status(), StatusCode::ACCEPTED);
        let start_body: Value = start.json().await.unwrap();
        let sovd_execution_id = start_body["id"].as_str().unwrap();

        let mut sovd_status = Value::Null;
        for _ in 0..100 {
            let response = client
                .get(format!(
                    "{base}/sovd/v1/components/ECM/operations/clear-faults/executions/{sovd_execution_id}"
                ))
                .send()
                .await
                .expect("SOVD execution status request must complete");
            sovd_status = response.json().await.unwrap();
            if sovd_status["status"] == "failed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert_eq!(sovd_status["status"], "failed");
        assert_eq!(
            sovd_status["error"][0]["error"]["error_code"],
            "uds-negative-response"
        );
        assert_eq!(
            sovd_status["error"][0]["error"]["vendor_code"],
            "UDS_NRC_0x31"
        );

        central_task.abort();
        mock_task.abort();
    }
}
