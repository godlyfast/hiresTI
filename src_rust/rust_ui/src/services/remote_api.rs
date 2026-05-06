//! Tiny HTTP JSON-RPC remote-control service. Mirrors the most-used
//! subset of the Python `services.remote_api` + `remote_dispatch`:
//! transport commands, queue read, search-tracks-by-id is deferred
//! until the queue-mutation pipeline ports across.
//!
//! Architecture: a dedicated thread owns a `tiny_http::Server`
//! bound to `127.0.0.1:<port>` (loopback by default). Each request
//! is dispatched on the same thread — the JSON-RPC handlers read
//! from a `Arc<Mutex<RemoteSnapshot>>` for state, and send
//! `AppInput` through the relm4 sender for transport commands.
//!
//! Auth: optional bearer token from `settings.remote_api_token`.
//! Origin allow-list ports the Python "client_allowed" CIDR check.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::thread;

use relm4::Sender;
use serde_json::{json, Value};
use tiny_http::{Header, Method, Response, Server};

use crate::messages::AppInput;

/// Snapshot of the playback state mutated from the GTK side.
/// Read-only from the HTTP thread.
#[derive(Debug, Clone, Default)]
pub struct RemoteSnapshot {
    pub logged_in: bool,
    pub is_playing: bool,
    pub is_paused: bool,
    pub position_seconds: f64,
    pub duration_seconds: f64,
    pub current_track: Option<RemoteTrack>,
    pub queue: Vec<RemoteTrack>,
    pub current_index: usize,
    pub volume_percent: u32,
}

#[derive(Debug, Clone, Default)]
pub struct RemoteTrack {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_seconds: u32,
    pub artist_id: i64,
    pub album_id: i64,
    pub cover: String,
}

#[derive(Debug, Clone, Default)]
pub struct RemoteConfig {
    pub enabled: bool,
    pub port: u16,
    pub token: String,
    pub allowed_cidrs: Vec<String>,
}

#[derive(Clone)]
pub struct RemoteHandle {
    state: Arc<Mutex<RemoteSnapshot>>,
}

impl RemoteHandle {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RemoteSnapshot::default())),
        }
    }

    pub fn update(&self, f: impl FnOnce(&mut RemoteSnapshot)) {
        if let Ok(mut s) = self.state.lock() {
            f(&mut s);
        }
    }

    fn snapshot(&self) -> RemoteSnapshot {
        self.state
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }
}

/// Owned server handle. Drop = shutdown thread.
pub struct RemoteServerHandle {
    /// Set to true by `Drop` so the accept loop bails.
    shutdown: Arc<std::sync::atomic::AtomicBool>,
    /// The tiny_http Server handle is held by the worker thread; we
    /// keep its address for an outgoing self-connect so accept()
    /// returns even when no real client is mid-flight.
    server_addr: String,
    join: Option<thread::JoinHandle<()>>,
}

impl Drop for RemoteServerHandle {
    fn drop(&mut self) {
        self.shutdown
            .store(true, std::sync::atomic::Ordering::SeqCst);
        // Best-effort wakeup: open a TCP connection to ourselves so
        // the blocking accept() returns and the loop notices the
        // shutdown flag. Failures are fine — the OS will reap the
        // thread when the process exits.
        if !self.server_addr.is_empty() {
            let _ = std::net::TcpStream::connect_timeout(
                &self.server_addr.parse().unwrap_or_else(|_| {
                    "127.0.0.1:0".parse().expect("loopback addr literal")
                }),
                std::time::Duration::from_millis(200),
            );
        }
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Start the HTTP server on `127.0.0.1:port`. Returns Err when the
/// port is already in use; the caller should log + skip.
pub fn start(
    handle: RemoteHandle,
    sender: Sender<AppInput>,
    cfg: RemoteConfig,
) -> Result<RemoteServerHandle, String> {
    let addr = format!("127.0.0.1:{}", cfg.port.max(1));
    let server = Server::http(&addr).map_err(|e| format!("remote-api bind {addr}: {e}"))?;
    let server_addr = server
        .server_addr()
        .to_ip()
        .map(|sa| sa.to_string())
        .unwrap_or_default();
    let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shutdown_for_thread = Arc::clone(&shutdown);
    let port = cfg.port;
    let cfg_for_thread = cfg;

    let join = thread::Builder::new()
        .name("remote-api".into())
        .spawn(move || {
            for req in server.incoming_requests() {
                if shutdown_for_thread.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                handle_request(req, &handle, &sender, &cfg_for_thread);
            }
        })
        .map_err(|e| format!("remote-api thread spawn: {e}"))?;

    tracing::info!(port, "remote-api: listening");
    Ok(RemoteServerHandle {
        shutdown,
        server_addr,
        join: Some(join),
    })
}

fn handle_request(
    mut req: tiny_http::Request,
    handle: &RemoteHandle,
    sender: &Sender<AppInput>,
    cfg: &RemoteConfig,
) {
    if !req.method().eq(&Method::Post) {
        let _ = req.respond(text_response(405, "method not allowed"));
        return;
    }
    if !path_is_rpc(req.url()) {
        let _ = req.respond(text_response(404, "not found"));
        return;
    }
    if !client_allowed(req.remote_addr(), &cfg.allowed_cidrs) {
        let _ = req.respond(text_response(403, "forbidden"));
        return;
    }
    if !cfg.token.is_empty() && !bearer_matches(req.headers(), &cfg.token) {
        let _ = req.respond(text_response(401, "unauthorized"));
        return;
    }

    let mut body = String::new();
    if req.as_reader().read_to_string(&mut body).is_err() {
        let _ = req.respond(text_response(400, "bad request"));
        return;
    }
    let payload: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => {
            let _ = req.respond(rpc_error_response(None, -32700, "Parse error"));
            return;
        }
    };

    let id = payload.get("id").cloned();
    let method = payload
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let params = payload.get("params").cloned().unwrap_or(Value::Null);

    let snapshot = handle.snapshot();
    let response = dispatch(&method, &params, &snapshot, sender);
    let body = match response {
        Ok(result) => json!({
            "jsonrpc": "2.0",
            "id": id.unwrap_or(Value::Null),
            "result": result,
        }),
        Err((code, msg)) => json!({
            "jsonrpc": "2.0",
            "id": id.unwrap_or(Value::Null),
            "error": { "code": code, "message": msg },
        }),
    };
    let body_str = body.to_string();
    let _ = req.respond(json_response(200, body_str));
}

fn dispatch(
    method: &str,
    params: &Value,
    snap: &RemoteSnapshot,
    sender: &Sender<AppInput>,
) -> Result<Value, (i32, String)> {
    match method {
        "ping" => Ok(json!({"ok": true, "app": "hiresTI"})),
        "auth.status" => Ok(json!({
            "logged_in": snap.logged_in,
            "remote_control_enabled": true,
        })),
        "player.get_state" => Ok(player_state(snap)),
        "player.play" => {
            let _ = sender.send(AppInput::TransportPlay);
            Ok(Value::Bool(true))
        }
        "player.pause" => {
            let _ = sender.send(AppInput::TransportPause);
            Ok(Value::Bool(true))
        }
        "player.play_pause" => {
            let _ = sender.send(AppInput::TogglePlayPause);
            Ok(Value::Bool(true))
        }
        "player.next" => {
            let _ = sender.send(AppInput::TransportNext);
            Ok(Value::Bool(true))
        }
        "player.previous" => {
            let _ = sender.send(AppInput::TransportPrev);
            Ok(Value::Bool(true))
        }
        "player.stop" => {
            // No dedicated Stop event in AppInput yet — pause is the
            // closest no-op friendly translation.
            let _ = sender.send(AppInput::TransportPause);
            Ok(Value::Bool(true))
        }
        "player.seek" => {
            let frac = params
                .get("position_fraction")
                .and_then(|v| v.as_f64())
                .or_else(|| {
                    params
                        .get("position_seconds")
                        .and_then(|v| v.as_f64())
                        .map(|s| {
                            if snap.duration_seconds > 0.0 {
                                (s / snap.duration_seconds).clamp(0.0, 1.0)
                            } else {
                                0.0
                            }
                        })
                });
            let Some(frac) = frac else {
                return Err((-32602, "Invalid params: need position_fraction or position_seconds".into()));
            };
            let _ = sender.send(AppInput::TransportSeek(frac.clamp(0.0, 1.0)));
            Ok(Value::Bool(true))
        }
        "queue.get" => Ok(json!({
            "current_index": snap.current_index,
            "queue_size": snap.queue.len(),
            "tracks": snap.queue.iter().map(serialize_track).collect::<Vec<_>>(),
        })),
        _ => Err((-32601, format!("Method not found: {method}"))),
    }
}

fn player_state(snap: &RemoteSnapshot) -> Value {
    json!({
        "is_playing": snap.is_playing,
        "is_paused": snap.is_paused,
        "position_seconds": snap.position_seconds,
        "duration_seconds": snap.duration_seconds,
        "track": snap.current_track.as_ref().map(serialize_track),
        "queue": snap.queue.iter().map(serialize_track).collect::<Vec<_>>(),
        "current_index": snap.current_index,
        "queue_size": snap.queue.len(),
        "volume_percent": snap.volume_percent,
    })
}

fn serialize_track(t: &RemoteTrack) -> Value {
    json!({
        "id": t.id.to_string(),
        "title": t.title,
        "artist": t.artist,
        "album": t.album,
        "duration_seconds": t.duration_seconds,
        "artist_id": if t.artist_id != 0 { t.artist_id.to_string() } else { String::new() },
        "album_id": if t.album_id != 0 { t.album_id.to_string() } else { String::new() },
        "cover": t.cover,
    })
}

fn path_is_rpc(url: &str) -> bool {
    let path = url.split('?').next().unwrap_or("");
    matches!(path, "/" | "/rpc" | "/jsonrpc")
}

fn client_allowed(addr: Option<&SocketAddr>, allowed: &[String]) -> bool {
    if allowed.is_empty() {
        return true;
    }
    let Some(addr) = addr else {
        return false;
    };
    let ip: IpAddr = addr.ip();
    for cidr in allowed {
        if cidr_contains(cidr, ip) {
            return true;
        }
    }
    false
}

/// Tiny CIDR matcher — handles plain `1.2.3.4` (host) and
/// `1.2.3.0/24` (network). Avoids pulling the full `ipnetwork` /
/// `ipnet` crate for one match in the hot path.
fn cidr_contains(cidr: &str, ip: IpAddr) -> bool {
    match cidr.split_once('/') {
        None => cidr.parse::<IpAddr>().map(|p| p == ip).unwrap_or(false),
        Some((net, prefix)) => {
            let Ok(prefix) = prefix.parse::<u8>() else {
                return false;
            };
            let Ok(net_addr) = net.parse::<IpAddr>() else {
                return false;
            };
            match (net_addr, ip) {
                (IpAddr::V4(n), IpAddr::V4(i)) => mask_v4(n.octets(), prefix) == mask_v4(i.octets(), prefix),
                (IpAddr::V6(n), IpAddr::V6(i)) => mask_v6(n.octets(), prefix) == mask_v6(i.octets(), prefix),
                _ => false,
            }
        }
    }
}

fn mask_v4(octets: [u8; 4], prefix: u8) -> [u8; 4] {
    let mut out = [0u8; 4];
    let bits = prefix.min(32);
    for (i, byte) in octets.iter().enumerate() {
        let shift = i as u32 * 8;
        let keep = bits as u32;
        if keep >= shift + 8 {
            out[i] = *byte;
        } else if keep <= shift {
            out[i] = 0;
        } else {
            let kept = keep - shift;
            out[i] = byte & (0xff_u8 << (8 - kept));
        }
    }
    out
}

fn mask_v6(octets: [u8; 16], prefix: u8) -> [u8; 16] {
    let mut out = [0u8; 16];
    let bits = prefix.min(128);
    for (i, byte) in octets.iter().enumerate() {
        let shift = i as u32 * 8;
        let keep = bits as u32;
        if keep >= shift + 8 {
            out[i] = *byte;
        } else if keep <= shift {
            out[i] = 0;
        } else {
            let kept = keep - shift;
            out[i] = byte & (0xff_u8 << (8 - kept));
        }
    }
    out
}

fn bearer_matches(headers: &[Header], expected: &str) -> bool {
    for h in headers {
        if h.field.equiv("Authorization") {
            let v = h.value.as_str();
            if let Some(tok) = v.strip_prefix("Bearer ") {
                return tok.trim() == expected;
            }
        }
    }
    false
}

fn json_response(code: u16, body: String) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut resp = Response::from_string(body);
    if let Ok(h) = "Content-Type: application/json".parse::<Header>() {
        resp = resp.with_header(h);
    }
    resp.with_status_code(code)
}

fn text_response(code: u16, body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body.to_string()).with_status_code(code)
}

fn rpc_error_response(
    id: Option<Value>,
    code: i32,
    msg: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "error": { "code": code, "message": msg },
    })
    .to_string();
    json_response(200, body)
}
