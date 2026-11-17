//! Single-owner UDS dispatcher for one ECU.

use std::{
    error::Error,
    fmt,
    sync::mpsc as std_mpsc,
    thread::{self, JoinHandle},
    time::Duration,
};

use tokio::sync::mpsc;
use uds_protocol::UdsServer;

/// Initial demonstrator capacity; tune using measured load.
pub const DEFAULT_UDS_QUEUE_CAPACITY: usize = 16;

/// Initial demonstrator response deadline; tune using measured latency.
const UDS_RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);

struct UdsRequest {
    payload: Vec<u8>,
    response_tx: std_mpsc::SyncSender<Option<Vec<u8>>>,
}

#[derive(Clone)]
pub struct UdsDispatcherHandle {
    sender: mpsc::Sender<UdsRequest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UdsDispatchError {
    ActorUnavailable,
    QueueFull,
    ResponseUnavailable,
    ResponseTimeout,
}

impl fmt::Display for UdsDispatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ActorUnavailable => write!(f, "UDS actor unavailable"),
            Self::QueueFull => write!(f, "UDS request queue full"),
            Self::ResponseUnavailable => write!(f, "UDS response channel unavailable"),
            Self::ResponseTimeout => write!(f, "UDS response timed out"),
        }
    }
}

impl Error for UdsDispatchError {}

fn receive_response(
    receiver: std_mpsc::Receiver<Option<Vec<u8>>>,
    timeout: Duration,
) -> Result<Option<Vec<u8>>, UdsDispatchError> {
    receiver.recv_timeout(timeout).map_err(|error| match error {
        std_mpsc::RecvTimeoutError::Timeout => UdsDispatchError::ResponseTimeout,
        std_mpsc::RecvTimeoutError::Disconnected => UdsDispatchError::ResponseUnavailable,
    })
}

impl UdsDispatcherHandle {
    /// Call only from a dedicated blocking OS thread,
    /// never from a Tokio runtime worker.
    pub fn process_blocking(&self, payload: Vec<u8>) -> Result<Option<Vec<u8>>, UdsDispatchError> {
        let (response_tx, response_rx) = std_mpsc::sync_channel(1);

        self.sender
            .try_send(UdsRequest {
                payload,
                response_tx,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => UdsDispatchError::QueueFull,
                mpsc::error::TrySendError::Closed(_) => UdsDispatchError::ActorUnavailable,
            })?;

        receive_response(response_rx, UDS_RESPONSE_TIMEOUT)
    }
}

pub fn spawn_uds_dispatcher(
    ecu_id: String,
    server: UdsServer,
    queue_capacity: usize,
) -> (UdsDispatcherHandle, JoinHandle<()>) {
    let (sender, mut receiver) = mpsc::channel::<UdsRequest>(queue_capacity.max(1));

    let actor_thread = thread::Builder::new()
        .name(format!("uds-{ecu_id}"))
        .spawn(move || {
            let mut server = server;

            while let Some(request) = receiver.blocking_recv() {
                let response = server.process_request(&request.payload);
                let _ = request.response_tx.send(response);
            }

            println!("[{ecu_id}][UDS] Dispatcher stopped");
        })
        .expect("failed to spawn UDS dispatcher");

    (UdsDispatcherHandle { sender }, actor_thread)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_wait_times_out() {
        let (_sender, receiver) = std_mpsc::sync_channel(1);

        let result = receive_response(receiver, Duration::from_millis(10));

        assert_eq!(result, Err(UdsDispatchError::ResponseTimeout));
    }

    #[test]
    fn disconnected_response_channel_is_reported() {
        let (sender, receiver) = std_mpsc::sync_channel(1);
        drop(sender);

        let result = receive_response(receiver, Duration::from_millis(10));

        assert_eq!(result, Err(UdsDispatchError::ResponseUnavailable));
    }

    #[test]
    fn blocking_request_is_processed() {
        let (dispatcher, actor) = spawn_uds_dispatcher(
            "TEST".to_owned(),
            UdsServer::new(),
            DEFAULT_UDS_QUEUE_CAPACITY,
        );

        let worker_handle = dispatcher.clone();
        let worker = thread::spawn(move || worker_handle.process_blocking(vec![0x99]).unwrap());

        assert_eq!(worker.join().unwrap(), Some(vec![0x7F, 0x99, 0x11]));

        drop(dispatcher);
        actor.join().unwrap();
    }
}
