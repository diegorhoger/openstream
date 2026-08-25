//! Deterministic fake OBS WebSocket v5 server for contract tests.
//!
//! A documented TEST SUPPORT component: an in-process loopback server
//! that speaks exactly the protocol subset the integration uses, so CI is
//! deterministic and OBS never needs to be installed. It is never a
//! production component and never binds anything except an ephemeral
//! 127.0.0.1 port while a test runs.
//!
//! Determinism model: each accepted connection performs the scripted
//! handshake (Hello, optional challenge auth with hash validation,
//! Identified), then loops: read one frame; if it is a request, look up
//! the scripted response and reply, then flush queued events in order;
//! if it is Identify, validate and acknowledge. Tests that need mid-run
//! events queue them first and then issue any request whose completion
//! proves the flush happened — no sleeps anywhere.
//!
//! Fault injection hooks (typed, bounded): require auth, announce wrong
//! versions, drop connections without answering a given request type
//! (`outcome_unknown` path), refuse authentication, and kill all live
//! connections (reconnect tests).

use crate::auth;
use crate::protocol;
use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

/// Scripted server behavior for every accepted connection.
#[derive(Debug, Clone)]
pub struct FakeObsConfig {
    /// Announced obsWebSocketVersion string.
    pub version: String,
    /// Announced rpcVersion.
    pub rpc_version: u32,
    /// When set, Hello demands authentication and the handshake validates
    /// the client hash against this password.
    pub password: Option<String>,
    /// When set, the server drops the connection WITHOUT responding to
    /// this request type (models disconnect mid-action).
    pub drop_on_request_type: Option<String>,
    /// When true, the server closes immediately after Identified even on
    /// valid handshakes (models flapping endpoints).
    pub close_after_identified: bool,
}

impl Default for FakeObsConfig {
    fn default() -> Self {
        Self {
            version: "5.4.2".to_string(),
            rpc_version: 1,
            password: None,
            drop_on_request_type: None,
            close_after_identified: false,
        }
    }
}

struct SharedScript {
    responses: Mutex<HashMap<String, serde_json::Value>>,
    events: Mutex<Vec<String>>,
    requests_seen: Mutex<Vec<(String, serde_json::Value)>>,
    connections_killed: Mutex<Vec<TcpStream>>,
}

/// A running fake OBS endpoint on an ephemeral loopback port.
pub struct FakeObsServer {
    addr: SocketAddr,
    script: Arc<SharedScript>,
}

impl FakeObsServer {
    /// Starts the accept loop on an ephemeral 127.0.0.1 port. Connections
    /// are served until [`FakeObsServer::shutdown`].
    #[must_use]
    pub fn start(config: FakeObsConfig) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback bind");
        let addr = listener.local_addr().expect("local addr");
        let script = Arc::new(SharedScript {
            responses: Mutex::new(HashMap::new()),
            events: Mutex::new(Vec::new()),
            requests_seen: Mutex::new(Vec::new()),
            connections_killed: Mutex::new(Vec::new()),
        });
        {
            let script = Arc::clone(&script);
            thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { break };
                    let script = Arc::clone(&script);
                    let config = config.clone();
                    thread::spawn(move || {
                        serve_connection(stream, &config, &script);
                    });
                }
            });
        }
        Self { addr, script }
    }

    /// The bound loopback address.
    #[must_use]
    pub const fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The host:port pair as a discovery candidate.
    #[must_use]
    pub fn candidate(&self) -> crate::discovery::DiscoveryCandidate {
        crate::discovery::DiscoveryCandidate::new("127.0.0.1", self.addr.port())
    }

    /// Scripts the response payload for one request type. `result: true`
    /// is implied unless the payload carries its own shape.
    pub fn respond_with(&self, request_type: &str, response_data: serde_json::Value) {
        self.script
            .responses
            .lock()
            .expect("scripted responses")
            .insert(request_type.to_string(), response_data);
    }

    /// Scripts a failing response (result false, typed code) for one
    /// request type.
    pub fn fail_requests_of(&self, request_type: &str, code: i64) {
        self.script
            .responses
            .lock()
            .expect("scripted responses")
            .insert(
                request_type.to_string(),
                serde_json::json!({ "__fail": code }),
            );
    }

    /// Queues one event frame for delivery before the next response (or
    /// immediately after Identified when queued pre-connect).
    pub fn queue_event(&self, event_type: &str, data: Option<&serde_json::Value>) {
        self.script
            .events
            .lock()
            .expect("queued events")
            .push(protocol::event_frame(event_type, data));
    }

    /// Every request the server observed across all connections, in
    /// arrival order: (requestType, requestData-or-null).
    #[must_use]
    pub fn recorded_requests(&self) -> Vec<(String, serde_json::Value)> {
        self.script
            .requests_seen
            .lock()
            .expect("recorded requests")
            .clone()
    }

    /// Clears the recorded request log.
    pub fn clear_requests(&self) {
        self.script
            .requests_seen
            .lock()
            .expect("recorded requests")
            .clear();
    }

    /// Kills every currently tracked live connection without ceremony
    /// (models an OBS crash). New connections keep being accepted.
    pub fn kill_connections(&self) {
        let mut killed = self.script.connections_killed.lock().expect("live set");
        for stream in killed.drain(..) {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }
}

fn serve_connection(stream: TcpStream, config: &FakeObsConfig, script: &SharedScript) {
    let peer = match stream.try_clone() {
        Ok(peer) => peer,
        Err(_) => return,
    };
    script
        .connections_killed
        .lock()
        .expect("live set")
        .push(peer);

    let socket = match tungstenite::accept(stream) {
        Ok(socket) => socket,
        Err(_) => return,
    };
    let mut socket = socket;
    let mut hello = serde_json::Map::new();
    hello.insert(
        "obsWebSocketVersion".to_string(),
        serde_json::Value::String(config.version.clone()),
    );
    hello.insert(
        "rpcVersion".to_string(),
        serde_json::Value::from(config.rpc_version),
    );
    if config.password.is_some() {
        hello.insert(
            "authentication".to_string(),
            serde_json::json!({ "challenge": "Y2hhbGxlbmdl", "salt": "c2FsdA==" }),
        );
    }
    send_text(
        &mut socket,
        &serde_json::json!({ "op": 0u8, "d": hello }).to_string(),
    );

    loop {
        let incoming = match read_text(&mut socket) {
            Some(text) => text,
            None => return,
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&incoming) else {
            return;
        };
        let op = value
            .get("op")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(255);
        let d = value.get("d").cloned().unwrap_or(serde_json::Value::Null);
        match op {
            1 => {
                let rpc_ok = d.get("rpcVersion").and_then(serde_json::Value::as_u64) == Some(1);
                let auth_ok = match (&config.password, d.get("authentication")) {
                    (None, _) => true,
                    (Some(password), Some(hash)) => hash.as_str() == Some(&auth_hash_for(password)),
                    (Some(_), None) => false,
                };
                if !rpc_ok || !auth_ok {
                    return; // drop: models OBS refusing bad credentials
                }
                send_text(&mut socket, &protocol::identified_frame(config.rpc_version));
                if config.close_after_identified {
                    return;
                }
                flush_events(&mut socket, script);
            }
            5 => {
                let request_type = d
                    .get("requestType")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let request_id = d
                    .get("requestId")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                script
                    .requests_seen
                    .lock()
                    .expect("recorded requests")
                    .push((
                        request_type.clone(),
                        d.get("requestData")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null),
                    ));
                if config.drop_on_request_type.as_deref() == Some(request_type.as_str()) {
                    return; // vanish without answering: outcome unknown
                }
                let scripted = script
                    .responses
                    .lock()
                    .expect("scripted responses")
                    .get(&request_type)
                    .cloned();
                // Events queued before this request flush BEFORE the
                // response, so a completed request implies its events were
                // observed — deterministic without sleeps.
                flush_events(&mut socket, script);
                let frame = match scripted {
                    Some(payload) if payload.get("__fail").is_some() => protocol::response_frame(
                        &request_type,
                        &request_id,
                        false,
                        payload
                            .get("__fail")
                            .and_then(serde_json::Value::as_i64)
                            .unwrap_or(100),
                        None,
                    ),
                    Some(payload) => protocol::response_frame(
                        &request_type,
                        &request_id,
                        true,
                        100,
                        Some(&payload),
                    ),
                    None => protocol::response_frame(&request_type, &request_id, true, 100, None),
                };
                send_text(&mut socket, &frame);
            }
            _ => return,
        }
    }
}

fn auth_hash_for(password: &str) -> String {
    auth::challenge_response(password, "c2FsdA==", "Y2hhbGxlbmdl")
}

fn flush_events(socket: &mut tungstenite::WebSocket<TcpStream>, script: &SharedScript) {
    let pending: Vec<String> = {
        let mut events = script.events.lock().expect("queued events");
        std::mem::take(&mut *events)
    };
    for frame in pending {
        send_text(socket, &frame);
    }
}

fn send_text(socket: &mut tungstenite::WebSocket<TcpStream>, text: &str) {
    use tungstenite::Message;
    let _ = socket.send(Message::text(text));
    let _ = socket.flush();
}

fn read_text(socket: &mut tungstenite::WebSocket<TcpStream>) -> Option<String> {
    use tungstenite::Message;
    loop {
        match socket.read() {
            Ok(Message::Text(text)) => return Some(text.to_string()),
            Ok(Message::Ping(payload)) => {
                let _ = socket.send(Message::Pong(payload));
            }
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => {}
        }
    }
}
