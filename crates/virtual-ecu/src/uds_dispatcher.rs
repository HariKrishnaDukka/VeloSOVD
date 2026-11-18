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
        self.process_blocking_with_timeout(payload, UDS_RESPONSE_TIMEOUT)
    }

    fn process_blocking_with_timeout(
        &self,
        payload: Vec<u8>,
        timeout: Duration,
    ) -> Result<Option<Vec<u8>>, UdsDispatchError> {
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

        receive_response(response_rx, timeout)
    }
}

pub fn spawn_uds_dispatcher(
    ecu_id: String,
    server: UdsServer,
    queue_capacity: usize,
) -> (UdsDispatcherHandle, JoinHandle<()>) {
    let mut server = server;
    spawn_uds_dispatcher_with(ecu_id, queue_capacity, move |payload| {
        server.process_request(payload)
    })
}

fn spawn_uds_dispatcher_with<F>(
    ecu_id: String,
    queue_capacity: usize,
    mut process: F,
) -> (UdsDispatcherHandle, JoinHandle<()>)
where
    F: FnMut(&[u8]) -> Option<Vec<u8>> + Send + 'static,
{
    let (sender, mut receiver) = mpsc::channel::<UdsRequest>(queue_capacity.max(1));

    let actor_thread = thread::Builder::new()
        .name(format!("uds-{ecu_id}"))
        .spawn(move || {
            while let Some(request) = receiver.blocking_recv() {
                let response = process(&request.payload);
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

    fn stalled_dispatcher(
        capacity: usize,
    ) -> (
        UdsDispatcherHandle,
        JoinHandle<()>,
        std_mpsc::Receiver<()>,
        std_mpsc::Sender<()>,
    ) {
        let (started_tx, started_rx) = std_mpsc::channel();
        let (release_tx, release_rx) = std_mpsc::channel();
        let mut first_request = true;

        let (dispatcher, actor) =
            spawn_uds_dispatcher_with("STALLED_TEST".to_owned(), capacity, move |_payload| {
                if first_request {
                    first_request = false;
                    let _ = started_tx.send(());
                    let _ = release_rx.recv();
                }
                Some(vec![0x54])
            });

        (dispatcher, actor, started_rx, release_tx)
    }

    #[test]
    fn stalled_actor_produces_deterministic_response_timeout() {
        let (dispatcher, actor, started, release) = stalled_dispatcher(1);
        let worker_handle = dispatcher.clone();

        let worker = thread::spawn(move || {
            worker_handle.process_blocking_with_timeout(
                vec![0x14, 0xFF, 0xFF, 0xFF],
                Duration::from_millis(20),
            )
        });

        started.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(
            worker.join().unwrap(),
            Err(UdsDispatchError::ResponseTimeout)
        );

        release.send(()).unwrap();
        drop(dispatcher);
        actor.join().unwrap();
    }

    #[test]
    fn stalled_actor_reports_queue_full_after_all_sixteen_slots_are_used() {
        let (dispatcher, actor, started, release) = stalled_dispatcher(DEFAULT_UDS_QUEUE_CAPACITY);
        let first_handle = dispatcher.clone();

        let first = thread::spawn(move || {
            first_handle.process_blocking_with_timeout(
                vec![0x14, 0xFF, 0xFF, 0xFF],
                Duration::from_secs(30),
            )
        });

        started.recv_timeout(Duration::from_secs(1)).unwrap();

        let mut queued_workers = Vec::with_capacity(DEFAULT_UDS_QUEUE_CAPACITY);
        for _ in 0..DEFAULT_UDS_QUEUE_CAPACITY {
            let handle = dispatcher.clone();
            queued_workers.push(thread::spawn(move || {
                handle.process_blocking_with_timeout(
                    vec![0x14, 0xFF, 0xFF, 0xFF],
                    Duration::from_secs(30),
                )
            }));
        }

        let capacity_deadline = std::time::Instant::now() + Duration::from_secs(1);
        while dispatcher.sender.capacity() != 0 && std::time::Instant::now() < capacity_deadline {
            thread::yield_now();
        }
        assert_eq!(dispatcher.sender.capacity(), 0);

        assert_eq!(
            dispatcher.process_blocking_with_timeout(
                vec![0x14, 0xFF, 0xFF, 0xFF],
                Duration::from_secs(1),
            ),
            Err(UdsDispatchError::QueueFull)
        );

        release.send(()).unwrap();
        assert_eq!(first.join().unwrap(), Ok(Some(vec![0x54])));
        for worker in queued_workers {
            assert_eq!(worker.join().unwrap(), Ok(Some(vec![0x54])));
        }

        drop(dispatcher);
        actor.join().unwrap();
    }
}
