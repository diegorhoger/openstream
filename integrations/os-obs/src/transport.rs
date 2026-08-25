//! Synchronous frame transport for the OBS WebSocket session.
//!
//! [`ObsTransport`] is the object-safe seam between the protocol/session
//! layer and any byte-carrying channel. The real implementation,
//! [`TungsteniteTransport`], speaks a text-mode WebSocket over TCP through
//! the pinned audited `tungstenite` dependency (synchronous API; no async
//! runtime is introduced anywhere in this crate, keeping the engine's
//! deterministic scheduling untouched).
//!
//! [`FakeTransport`] scripts frame sequences in memory for unit tests; it
//! is a documented test double and never a production fallback.
//!
//! Error values carry structural classes only. No host, port, or OS error
//! text enters any error value.

use core::fmt;
use std::net::TcpStream;
use std::time::{Duration, Instant};

/// Structural transport failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    /// The TCP connection or WebSocket handshake could not be established.
    ConnectFailed,
    /// The peer closed the connection (or the socket was already dead).
    Closed,
    /// The operation exceeded its deadline.
    Timeout,
    /// A non-timeout I/O failure occurred mid-operation.
    IoFailure,
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ConnectFailed => "connection failed",
            Self::Closed => "connection closed",
            Self::Timeout => "operation timed out",
            Self::IoFailure => "transport i/o failure",
        })
    }
}

impl std::error::Error for TransportError {}

/// Object-safe synchronous frame transport. Text frames only: OBS
/// WebSocket v5 carries JSON exclusively.
pub trait ObsTransport: fmt::Debug + Send {
    /// Sends one text frame.
    ///
    /// # Errors
    /// [`TransportError`] on failure to deliver the frame.
    fn send_text(&mut self, text: &str) -> Result<(), TransportError>;

    /// Receives one text frame, waiting at most `timeout_ms`.
    ///
    /// # Errors
    /// [`TransportError::Closed`] when the peer closes; [`TransportError::Timeout`]
    /// when the deadline elapses first.
    fn receive_text(&mut self, timeout_ms: u64) -> Result<String, TransportError>;
}

/// Real WebSocket transport over TCP (`ws://` plaintext on the trusted
/// local segment; OBS itself ships no TLS option for its WebSocket server).
#[derive(Debug)]
pub struct TungsteniteTransport {
    socket: tungstenite::WebSocket<TcpStream>,
}

impl TungsteniteTransport {
    /// Connects to `host:port` with bounded connect and handshake time.
    ///
    /// # Errors
    /// [`TransportError::ConnectFailed`] on DNS/TCP/handshake failure.
    pub fn connect(host: &str, port: u16, timeout_ms: u64) -> Result<Self, TransportError> {
        let timeout = Duration::from_millis(timeout_ms);
        let addresses: Vec<std::net::SocketAddr> =
            match std::net::ToSocketAddrs::to_socket_addrs(&(host, port)) {
                Ok(iter) => iter.collect(),
                Err(_) => return Err(TransportError::ConnectFailed),
            };
        if addresses.is_empty() {
            return Err(TransportError::ConnectFailed);
        }
        let mut last = TransportError::ConnectFailed;
        for address in addresses {
            match TcpStream::connect_timeout(&address, timeout) {
                Ok(stream) => {
                    let _ = stream.set_nodelay(true);
                    stream
                        .set_write_timeout(Some(timeout))
                        .map_err(|_| TransportError::ConnectFailed)?;
                    let url = format!("ws://{host}:{port}");
                    let request = tungstenite::client::IntoClientRequest::into_client_request(url)
                        .map_err(|_| TransportError::ConnectFailed)?;
                    let socket = tungstenite::client(request, stream)
                        .map(|(socket, _response)| socket)
                        .map_err(|error| match error {
                            tungstenite::HandshakeError::Failure(tungstenite::Error::Http(_)) => {
                                TransportError::ConnectFailed
                            }
                            _ => TransportError::IoFailure,
                        })?;
                    return Ok(Self { socket });
                }
                Err(_) => {
                    last = TransportError::ConnectFailed;
                }
            }
        }
        Err(last)
    }

    /// Best-effort close: sends a Close frame, ignoring failures.
    pub fn send_close(&mut self) {
        let _ = self.socket.send(tungstenite::Message::Close(None));
        let _ = self.socket.close(None);
    }

    /// The underlying TCP stream for callers that must force-shutdown.
    #[must_use]
    pub fn raw_stream(&mut self) -> Option<&TcpStream> {
        Some(self.socket.get_ref())
    }
}

impl ObsTransport for TungsteniteTransport {
    fn send_text(&mut self, text: &str) -> Result<(), TransportError> {
        self.socket
            .send(tungstenite::Message::text(text))
            .map(|_| ())
            .map_err(classify_message_error)
    }

    fn receive_text(&mut self, timeout_ms: u64) -> Result<String, TransportError> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(TransportError::Timeout);
            }
            let slice = std::cmp::min(deadline - now, Duration::from_millis(250));
            self.socket
                .get_ref()
                .set_read_timeout(Some(slice))
                .map_err(|_| TransportError::IoFailure)?;
            match self.socket.read() {
                Ok(tungstenite::Message::Text(text)) => return Ok(text.to_string()),
                Ok(tungstenite::Message::Binary(_)) => {
                    // OBS v5 never sends binary frames; treat as hostile.
                    return Err(TransportError::Closed);
                }
                Ok(tungstenite::Message::Ping(payload)) => {
                    let _ = self.socket.send(tungstenite::Message::Pong(payload));
                }
                Ok(tungstenite::Message::Pong(_)) => {}
                Ok(tungstenite::Message::Close(_)) => return Err(TransportError::Closed),
                Ok(tungstenite::Message::Frame(_)) => {}
                Err(tungstenite::Error::Io(error))
                    if error.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    continue;
                }
                Err(tungstenite::Error::ConnectionClosed) => return Err(TransportError::Closed),
                Err(tungstenite::Error::AlreadyClosed) => return Err(TransportError::Closed),
                Err(_) => return Err(TransportError::IoFailure),
            }
        }
    }
}

fn classify_message_error(error: tungstenite::Error) -> TransportError {
    match error {
        tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => {
            TransportError::Closed
        }
        tungstenite::Error::Io(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            TransportError::Timeout
        }
        _ => TransportError::IoFailure,
    }
}

/// Scripted in-memory transport for unit tests. Records every sent frame
/// and replays queued receive results in order.
#[derive(Debug, Default)]
pub struct FakeTransport {
    sent: std::sync::Mutex<Vec<String>>,
    incoming: std::sync::Mutex<Vec<Result<String, TransportError>>>,
}

impl FakeTransport {
    /// Creates an empty scripted transport.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues one successful inbound frame.
    pub fn push_incoming(&self, frame: &str) {
        self.incoming
            .lock()
            .expect("scripted queue")
            .push(Ok(frame.to_string()));
    }

    /// Queues one inbound transport failure (for example a drop).
    pub fn push_failure(&self, error: TransportError) {
        self.incoming
            .lock()
            .expect("scripted queue")
            .push(Err(error));
    }

    /// Every frame sent through this transport, in order.
    #[must_use]
    pub fn sent(&self) -> Vec<String> {
        self.sent.lock().expect("sent log").clone()
    }
}

impl ObsTransport for FakeTransport {
    fn send_text(&mut self, text: &str) -> Result<(), TransportError> {
        self.sent.lock().expect("sent log").push(text.to_string());
        Ok(())
    }

    fn receive_text(&mut self, _timeout_ms: u64) -> Result<String, TransportError> {
        self.incoming
            .lock()
            .expect("scripted queue")
            .pop_front_or_closed()
    }
}

trait PopFrontOrClosed {
    fn pop_front_or_closed(&mut self) -> Result<String, TransportError>;
}

impl PopFrontOrClosed for Vec<Result<String, TransportError>> {
    fn pop_front_or_closed(&mut self) -> Result<String, TransportError> {
        if self.is_empty() {
            return Err(TransportError::Closed);
        }
        self.remove(0)
    }
}
