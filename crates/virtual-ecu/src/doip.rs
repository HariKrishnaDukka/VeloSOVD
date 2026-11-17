//! Zone-specific DoIP TCP listener.
//!
//! Initial implementation supports routing activation and diagnostic messages.
//! Unsupported payload types close the connection; ACK/NACK and vehicle
//! identification handling are intentionally not implemented yet.

use std::{io, sync::Arc};

use doip_gateway::{
    DiagnosticMessage, DiagnosticRouter, DoipPayloadType, DoipTransport, RoutingActivationRequest,
    RoutingActivationResponse,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Semaphore, watch},
    task::JoinSet,
};

const MAX_ACTIVE_CONNECTIONS: usize = 16;

pub async fn run_listener(
    listener: TcpListener,
    entity_logical_address: u16,
    router: Arc<DiagnosticRouter>,
    mut shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    let permits = Arc::new(Semaphore::new(MAX_ACTIVE_CONNECTIONS));
    let mut connections = JoinSet::new();

    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }

            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                let permit = match Arc::clone(&permits).try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        eprintln!("[DoIP] Rejecting {peer}: connection limit reached");
                        drop(stream);
                        continue;
                    }
                };

                let connection_shutdown = shutdown.clone();
                let connection_router = Arc::clone(&router);

                connections.spawn(async move {
                    let _permit = permit;
                    handle_connection(
                        stream,
                        peer.to_string(),
                        entity_logical_address,
                        connection_router,
                        connection_shutdown,
                    )
                    .await;
                });
            }

            Some(result) = connections.join_next(), if !connections.is_empty() => {
                if let Err(error) = result {
                    eprintln!("[DoIP] Connection task failed: {error}");
                }
            }
        }
    }

    // The shutdown watch signal is shared with every connection task.
    while let Some(result) = connections.join_next().await {
        if let Err(error) = result {
            eprintln!("[DoIP] Connection task failed during shutdown: {error}");
        }
    }

    Ok(())
}

async fn handle_connection(
    stream: TcpStream,
    peer: String,
    entity_logical_address: u16,
    router: Arc<DiagnosticRouter>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut transport = DoipTransport::new(stream);
    let mut routing = doip_gateway::RoutingConnection::new(entity_logical_address);

    println!("[DoIP] Client connected: {peer}");

    loop {
        if *shutdown.borrow() {
            break;
        }

        let frame = tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }

                continue;
            }
            received = transport.receive_frame() => {
                match received {
                    Ok(frame) => frame,
                    Err(error) => {
                        eprintln!("[DoIP] {peer}: receive failed: {error:?}");
                        break;
                    }
                }
            }
        };

        match DoipPayloadType::from_raw(frame.header.payload_type) {
            Some(DoipPayloadType::RoutingActivationRequest) => {
                let request = match RoutingActivationRequest::decode(&frame.payload) {
                    Ok(request) if request.reserved == [0; 4] => request,
                    _ => {
                        eprintln!("[DoIP] {peer}: invalid routing activation payload");
                        break;
                    }
                };

                let response_code = match routing.activate(&request) {
                    Ok(code) => code,
                    Err(error) => {
                        eprintln!("[DoIP] {peer}: routing activation rejected: {error:?}");
                        break;
                    }
                };

                let response = RoutingActivationResponse {
                    tester_logical_address: request.tester_logical_address,
                    entity_logical_address,
                    response_code,
                    reserved: [0; 4],
                };

                if let Err(error) = transport.send_frame(0x0006, &response.encode()).await {
                    eprintln!("[DoIP] {peer}: activation response failed: {error:?}");
                    break;
                }

                println!(
                    "[DoIP] {peer}: routing active for tester 0x{:04X}",
                    request.tester_logical_address
                );
            }

            Some(DoipPayloadType::DiagnosticMessage) => {
                let connection = routing;
                let payload = frame.payload;
                let route_router = Arc::clone(&router);

                // The UDS dispatcher is synchronous. Never block a Tokio worker.
                let routed =
                    tokio::task::spawn_blocking(move || route_router.route(&connection, &payload))
                        .await;

                let response = match routed {
                    Ok(Ok(response)) => response,
                    Ok(Err(error)) => {
                        eprintln!("[DoIP] {peer}: diagnostic routing failed: {error}");
                        break;
                    }
                    Err(error) => {
                        eprintln!("[DoIP] {peer}: blocking route task failed: {error}");
                        break;
                    }
                };

                if let Some(uds_payload) = response.uds_payload {
                    let response_payload = DiagnosticMessage {
                        source_address: response.source_address,
                        target_address: response.target_address,
                        uds_payload: &uds_payload,
                    }
                    .encode();

                    if let Err(error) = transport.send_frame(0x8001, &response_payload).await {
                        eprintln!("[DoIP] {peer}: diagnostic response failed: {error:?}");
                        break;
                    }
                }
            }

            other => {
                eprintln!(
                    "[DoIP] {peer}: unsupported payload type {:?}; closing connection",
                    other
                );
                break;
            }
        }
    }

    routing.close();
    println!("[DoIP] Client disconnected: {peer}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use doip_gateway::{
        ROUTING_RESPONSE_SUCCESS, RoutingActivationRequest, UdsDispatchFailure, UdsRequestHandler,
    };
    use tokio::{
        net::TcpListener,
        sync::watch,
        time::{Duration, timeout},
    };

    const TESTER_ADDRESS: u16 = 0x0E00;
    const ENTITY_ADDRESS: u16 = 0x1101;
    const ECU_ADDRESS: u16 = 0x1110;

    struct EchoHandler;

    impl UdsRequestHandler for EchoHandler {
        fn process_uds(&self, payload: &[u8]) -> Result<Option<Vec<u8>>, UdsDispatchFailure> {
            let mut response = payload.to_vec();

            if let Some(first) = response.first_mut() {
                *first = first.wrapping_add(0x40);
            }

            Ok(Some(response))
        }
    }

    async fn create_test_connection() -> (
        DoipTransport,
        tokio::task::JoinHandle<()>,
        watch::Sender<bool>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener bind failed");

        let address = listener.local_addr().unwrap();

        let client_stream = TcpStream::connect(address)
            .await
            .expect("test client connect failed");

        let (server_stream, peer) = listener.accept().await.expect("test server accept failed");

        let mut router = DiagnosticRouter::new();
        router
            .register(ECU_ADDRESS, Arc::new(EchoHandler))
            .expect("test ECU registration failed");

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let server_task = tokio::spawn(handle_connection(
            server_stream,
            peer.to_string(),
            ENTITY_ADDRESS,
            Arc::new(router),
            shutdown_rx,
        ));

        (DoipTransport::new(client_stream), server_task, shutdown_tx)
    }

    #[tokio::test]
    async fn tcp_routing_activation_and_uds_diagnostic_exchange() {
        let (mut client, server_task, shutdown_tx) = create_test_connection().await;

        // Step 1: Request diagnostic routing activation.
        let activation_request = RoutingActivationRequest {
            tester_logical_address: TESTER_ADDRESS,
            activation_type: 0x00,
            reserved: [0; 4],
        };

        client
            .send_frame(0x0005, &activation_request.encode())
            .await
            .expect("routing activation send failed");

        // Step 2: Verify routing activation response.
        let activation_frame = timeout(Duration::from_secs(2), client.receive_frame())
            .await
            .expect("routing activation response timed out")
            .expect("routing activation receive failed");

        assert_eq!(activation_frame.header.payload_type, 0x0006);

        let activation_response =
            doip_gateway::RoutingActivationResponse::decode(&activation_frame.payload)
                .expect("invalid routing activation response");

        assert_eq!(activation_response.tester_logical_address, TESTER_ADDRESS);
        assert_eq!(activation_response.entity_logical_address, ENTITY_ADDRESS);
        assert_eq!(activation_response.response_code, ROUTING_RESPONSE_SUCCESS);
        assert_eq!(activation_response.reserved, [0; 4]);

        // Step 3: Send UDS ReadDataByIdentifier (VIN DID F190).
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
            .expect("diagnostic request send failed");

        // Step 4: Verify the ECU diagnostic response.
        let diagnostic_frame = timeout(Duration::from_secs(2), client.receive_frame())
            .await
            .expect("diagnostic response timed out")
            .expect("diagnostic response receive failed");

        assert_eq!(diagnostic_frame.header.payload_type, 0x8001);

        let diagnostic_response = DiagnosticMessage::decode(&diagnostic_frame.payload)
            .expect("invalid diagnostic response");

        assert_eq!(diagnostic_response.source_address, ECU_ADDRESS);
        assert_eq!(diagnostic_response.target_address, TESTER_ADDRESS);
        assert_eq!(diagnostic_response.uds_payload, &[0x62, 0xF1, 0x90]);

        // Step 5: Shut down the connection task cleanly.
        shutdown_tx.send(true).expect("shutdown signal failed");

        timeout(Duration::from_secs(2), server_task)
            .await
            .expect("server task shutdown timed out")
            .expect("server task panicked");
    }

    #[tokio::test]
    async fn rejects_diagnostic_message_before_routing_activation() {
        let (mut client, server_task, _shutdown_tx) = create_test_connection().await;

        let diagnostic_request = DiagnosticMessage {
            source_address: TESTER_ADDRESS,
            target_address: ECU_ADDRESS,
            uds_payload: &[0x22, 0xF1, 0x90],
        }
        .encode();

        client
            .send_frame(0x8001, &diagnostic_request)
            .await
            .expect("diagnostic request send failed");

        // The listener closes the connection when routing is inactive.
        let receive_result = timeout(Duration::from_secs(2), client.receive_frame())
            .await
            .expect("connection was not closed after invalid request");

        assert!(
            receive_result.is_err(),
            "diagnostic response must not be sent before activation"
        );

        timeout(Duration::from_secs(2), server_task)
            .await
            .expect("server task shutdown timed out")
            .expect("server task panicked");
    }
}
