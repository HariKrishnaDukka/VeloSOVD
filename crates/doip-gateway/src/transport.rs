use std::io;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{Duration, timeout};

use crate::header::{DEFAULT_MAX_PAYLOAD_LENGTH, DoipHeader, HEADER_LENGTH, HeaderError};

#[derive(Debug)]
pub enum TransportError {
    Io(io::Error),
    Header(HeaderError),
    Timeout,
    PayloadLengthOverflow,
    ConnectionUnusable,
}

impl From<io::Error> for TransportError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<HeaderError> for TransportError {
    fn from(error: HeaderError) -> Self {
        Self::Header(error)
    }
}

pub struct DoipFrame {
    pub header: DoipHeader,
    pub payload: Vec<u8>,
}

pub struct DoipTransport {
    stream: TcpStream,
    max_payload_length: u32,
    io_timeout: Duration,
    unusable: bool,
}

impl DoipTransport {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            max_payload_length: DEFAULT_MAX_PAYLOAD_LENGTH,
            io_timeout: Duration::from_secs(5),
            unusable: false,
        }
    }

    pub fn with_limits(stream: TcpStream, max_payload_length: u32, io_timeout: Duration) -> Self {
        Self {
            stream,
            max_payload_length,
            io_timeout,
            unusable: false,
        }
    }

    pub async fn receive_frame(&mut self) -> Result<DoipFrame, TransportError> {
        if self.unusable {
            return Err(TransportError::ConnectionUnusable);
        }

        let result = async {
            let mut header_bytes = [0_u8; HEADER_LENGTH];

            timeout(self.io_timeout, self.stream.read_exact(&mut header_bytes))
                .await
                .map_err(|_| TransportError::Timeout)??;

            let header = DoipHeader::decode(&header_bytes, self.max_payload_length)?;

            let payload_length = usize::try_from(header.payload_length)
                .map_err(|_| TransportError::PayloadLengthOverflow)?;

            let mut payload = vec![0_u8; payload_length];

            if payload_length > 0 {
                timeout(self.io_timeout, self.stream.read_exact(&mut payload))
                    .await
                    .map_err(|_| TransportError::Timeout)??;
            }

            Ok(DoipFrame { header, payload })
        }
        .await;

        if result.is_err() {
            self.unusable = true;
        }

        result
    }

    pub async fn send_frame(
        &mut self,
        payload_type: u16,
        payload: &[u8],
    ) -> Result<(), TransportError> {
        if self.unusable {
            return Err(TransportError::ConnectionUnusable);
        }

        let payload_length =
            u32::try_from(payload.len()).map_err(|_| TransportError::PayloadLengthOverflow)?;

        if payload_length > self.max_payload_length {
            return Err(TransportError::Header(HeaderError::PayloadTooLarge));
        }

        let header = DoipHeader::new(payload_type, payload_length);
        let header_bytes = header.encode();

        let result = timeout(self.io_timeout, async {
            self.stream.write_all(&header_bytes).await?;
            self.stream.write_all(payload).await?;
            self.stream.flush().await
        })
        .await
        .map_err(|_| TransportError::Timeout)
        .and_then(|result| result.map_err(TransportError::Io));

        if result.is_err() {
            self.unusable = true;
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio::time::sleep;

    async fn create_tcp_pair() -> (DoipTransport, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind test listener");

        let address = listener.local_addr().unwrap();

        let client = TcpStream::connect(address)
            .await
            .expect("Failed to connect test client");

        let (server, _) = listener
            .accept()
            .await
            .expect("Failed to accept test connection");

        (DoipTransport::new(server), client)
    }

    #[tokio::test]
    async fn receives_complete_frame() {
        let (mut transport, mut client) = create_tcp_pair().await;

        let header = DoipHeader::new(0x8001, 3).encode();

        client.write_all(&header).await.unwrap();
        client.write_all(&[0x22, 0xF1, 0x90]).await.unwrap();

        let frame = transport.receive_frame().await.unwrap();

        assert_eq!(frame.header.payload_type, 0x8001);
        assert_eq!(frame.payload, vec![0x22, 0xF1, 0x90]);
    }

    #[tokio::test]
    async fn receives_fragmented_header_and_payload() {
        let (mut transport, mut client) = create_tcp_pair().await;

        let header = DoipHeader::new(0x8001, 4).encode();

        let writer = tokio::spawn(async move {
            client.write_all(&header[..3]).await.unwrap();
            sleep(Duration::from_millis(10)).await;

            client.write_all(&header[3..]).await.unwrap();
            sleep(Duration::from_millis(10)).await;

            client.write_all(&[0x22, 0xF1]).await.unwrap();
            sleep(Duration::from_millis(10)).await;

            client.write_all(&[0x90, 0x01]).await.unwrap();
        });

        let frame = transport.receive_frame().await.unwrap();

        writer.await.unwrap();

        assert_eq!(frame.header.payload_type, 0x8001);
        assert_eq!(frame.payload, vec![0x22, 0xF1, 0x90, 0x01]);
    }

    #[tokio::test]
    async fn receives_zero_length_payload() {
        let (mut transport, mut client) = create_tcp_pair().await;

        let header = DoipHeader::new(0x0007, 0).encode();

        client.write_all(&header).await.unwrap();

        let frame = transport.receive_frame().await.unwrap();

        assert_eq!(frame.header.payload_length, 0);
        assert!(frame.payload.is_empty());
    }

    #[tokio::test]
    async fn sends_complete_frame() {
        let (mut transport, mut client) = create_tcp_pair().await;

        transport
            .send_frame(0x8001, &[0x62, 0xF1, 0x90])
            .await
            .unwrap();

        let mut header_bytes = [0_u8; HEADER_LENGTH];
        client.read_exact(&mut header_bytes).await.unwrap();

        let header = DoipHeader::decode(&header_bytes, DEFAULT_MAX_PAYLOAD_LENGTH).unwrap();

        let mut payload = vec![0_u8; header.payload_length as usize];
        client.read_exact(&mut payload).await.unwrap();

        assert_eq!(header.payload_type, 0x8001);
        assert_eq!(payload, vec![0x62, 0xF1, 0x90]);
    }

    #[tokio::test]
    async fn rejects_oversized_outgoing_payload() {
        let (mut transport, _client) = create_tcp_pair().await;

        let result = transport.send_frame(0x8001, &[0_u8; 4097]).await;

        assert!(matches!(
            result,
            Err(TransportError::Header(HeaderError::PayloadTooLarge))
        ));
    }

    #[tokio::test]
    async fn rejects_invalid_header() {
        let (mut transport, mut client) = create_tcp_pair().await;

        let mut header = DoipHeader::new(0x8001, 0).encode();

        // Deliberately corrupt the protocol version.
        header[0] = 0x03;

        client.write_all(&header).await.unwrap();

        let result = transport.receive_frame().await;

        assert!(matches!(result, Err(TransportError::Header(_))));
    }

    #[tokio::test]
    async fn detects_disconnect_during_header() {
        let (mut transport, mut client) = create_tcp_pair().await;

        client.write_all(&[0x02, 0xFD, 0x80]).await.unwrap();

        drop(client);

        let result = transport.receive_frame().await;

        assert!(matches!(result, Err(TransportError::Io(_))));
    }

    #[tokio::test]
    async fn detects_disconnect_during_payload() {
        let (mut transport, mut client) = create_tcp_pair().await;

        let header = DoipHeader::new(0x8001, 4).encode();

        client.write_all(&header).await.unwrap();
        client.write_all(&[0x22, 0xF1]).await.unwrap();

        drop(client);

        let result = transport.receive_frame().await;

        assert!(matches!(result, Err(TransportError::Io(_))));
    }

    #[tokio::test]
    async fn times_out_on_incomplete_header() {
        let (server, _client) = create_tcp_pair().await;

        let mut transport = DoipTransport::with_limits(
            server.stream,
            DEFAULT_MAX_PAYLOAD_LENGTH,
            Duration::from_millis(50),
        );

        let result = transport.receive_frame().await;

        assert!(matches!(result, Err(TransportError::Timeout)));
    }

    #[tokio::test]
    async fn connection_is_unusable_after_receive_timeout() {
        let (server, _client) = create_tcp_pair().await;

        let mut transport = DoipTransport::with_limits(
            server.stream,
            DEFAULT_MAX_PAYLOAD_LENGTH,
            Duration::from_millis(50),
        );

        let first = transport.receive_frame().await;
        assert!(matches!(first, Err(TransportError::Timeout)));

        let second = transport.receive_frame().await;
        assert!(matches!(second, Err(TransportError::ConnectionUnusable)));
    }
}
