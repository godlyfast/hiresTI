//! Scrobbler service for Last.fm and ListenBrainz. Mirrors the Python
//! `services.scrobbler.ScrobblerService` 1:1 — same submission rules,
//! same Last.fm v2 signing, same ListenBrainz playing_now / single
//! payload shapes.
//!
//! Submission rules (Last.fm spec): a track is scrobbled when
//! `position >= 30s AND position >= min(duration / 2, 240s)`.
//!
//! All HTTP calls run on detached worker threads via `spawn_blocking`
//! so the GTK main thread is never blocked. The service holds no
//! Tidal-typed handles itself — the caller passes the bare track
//! fields it wants submitted.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use md5::{Digest, Md5};

use crate::services::tidal_session::spawn_blocking;

const LASTFM_API_URL: &str = "https://ws.audioscrobbler.com/2.0/";
const LISTENBRAINZ_API_URL: &str = "https://api.listenbrainz.org/1/submit-listens";
const HTTP_TIMEOUT_SECS: u64 = 10;

/// Last.fm web API key/secret. Same constants as the Python build —
/// they were registered for hiresti, not borrowed from another app.
const LASTFM_API_KEY: &str = "24f06b8f7dbc89a0061d0c9a0f450eec";
const LASTFM_API_SECRET: &str = "1ddd21eee354659d148d19736255c9e5";

/// Configuration pulled from `settings.extra`. Empty fields disable
/// the corresponding backend; both can be active simultaneously.
#[derive(Debug, Clone, Default)]
pub struct ScrobblerConfig {
    pub lastfm_enabled: bool,
    pub lastfm_session_key: String,
    pub listenbrainz_enabled: bool,
    pub listenbrainz_token: String,
}

impl ScrobblerConfig {
    pub fn lastfm_active(&self) -> bool {
        self.lastfm_enabled && !self.lastfm_session_key.is_empty()
    }

    pub fn listenbrainz_active(&self) -> bool {
        self.listenbrainz_enabled && !self.listenbrainz_token.is_empty()
    }

    pub fn any_active(&self) -> bool {
        self.lastfm_active() || self.listenbrainz_active()
    }
}

/// Owned track fields the scrobbler submits. Kept decoupled from
/// `rust_tidal_core::api::Track` so this service stays trivially
/// reusable for non-Tidal sources later.
#[derive(Debug, Clone, Default)]
pub struct ScrobbleTrack {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Track length in seconds. 0 means "unknown" — submitted without
    /// the duration field.
    pub duration: u32,
}

/// In-flight scrobble state for the currently-playing track.
#[derive(Debug, Default)]
struct InFlight {
    track: Option<ScrobbleTrack>,
    /// UNIX timestamp of when the track started playing. Last.fm's
    /// scrobble payload reports this verbatim.
    started_at: i64,
    sent: bool,
}

#[derive(Debug, Default)]
struct State {
    config: ScrobblerConfig,
    in_flight: InFlight,
}

/// Cheaply-cloneable handle that the UI keeps in `AppController`.
/// All public methods are non-blocking — submissions go to detached
/// worker threads.
#[derive(Debug, Clone, Default)]
pub struct ScrobblerService {
    state: Arc<Mutex<State>>,
}

impl ScrobblerService {
    pub fn new() -> Self {
        Self::default()
    }

    /// Refresh the backend config from `settings.extra`. Safe to call
    /// every ApplySettings — it's a Mutex swap, not a thread spawn.
    pub fn configure(&self, cfg: ScrobblerConfig) {
        if let Ok(mut s) = self.state.lock() {
            s.config = cfg;
        }
    }

    /// Snapshot the active config. Useful for the Settings dialog
    /// when the user wants to "Test connection".
    pub fn config(&self) -> ScrobblerConfig {
        self.state
            .lock()
            .map(|s| s.config.clone())
            .unwrap_or_default()
    }

    /// Mark a new track as the playing one. Resets the scrobble
    /// guard + sends a "now playing" notification on a worker thread.
    pub fn on_track_started(&self, track: ScrobbleTrack) {
        let cfg = {
            let Ok(mut s) = self.state.lock() else { return };
            s.in_flight = InFlight {
                track: Some(track.clone()),
                started_at: now_unix(),
                sent: false,
            };
            s.config.clone()
        };

        if !cfg.any_active() {
            return;
        }
        if track.title.is_empty() || track.artist.is_empty() {
            return;
        }
        spawn_blocking(
            move || notify_now_playing(&cfg, &track),
            // Result is logged inside the worker; nothing to forward.
            |_| {},
        );
    }

    /// Drop the current track without submitting. Called on logout
    /// and on "skip without ever crossing the threshold" — e.g. user
    /// hits Next 5s in.
    pub fn on_track_stopped(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.in_flight = InFlight::default();
        }
    }

    /// Periodic tick from the playback timer. Submits the scrobble
    /// when the threshold is reached. No-op once the current track
    /// has already been scrobbled.
    pub fn tick(&self, position_s: f64, duration_s: f64) {
        let Ok(mut s) = self.state.lock() else { return };
        if s.in_flight.sent || s.in_flight.track.is_none() {
            return;
        }
        if !s.config.any_active() {
            return;
        }
        if duration_s <= 0.0 || position_s < 30.0 {
            return;
        }
        let threshold = (duration_s * 0.5).min(240.0);
        if position_s < threshold {
            return;
        }
        let cfg = s.config.clone();
        let track = s.in_flight.track.clone().unwrap_or_default();
        let started_at = s.in_flight.started_at;
        s.in_flight.sent = true;
        // Drop the lock before spawning so any reentrant tick can
        // run without contention.
        drop(s);
        spawn_blocking(
            move || submit_scrobble(&cfg, &track, started_at),
            |_| {},
        );
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn notify_now_playing(cfg: &ScrobblerConfig, track: &ScrobbleTrack) {
    if cfg.lastfm_active() {
        let mut params: Vec<(&str, String)> = vec![
            ("method", "track.updateNowPlaying".into()),
            ("track", track.title.clone()),
            ("artist", track.artist.clone()),
            ("album", track.album.clone()),
            ("api_key", LASTFM_API_KEY.into()),
            ("sk", cfg.lastfm_session_key.clone()),
        ];
        if track.duration > 0 {
            params.push(("duration", track.duration.to_string()));
        }
        let sig = lastfm_sign(&params, LASTFM_API_SECRET);
        params.push(("api_sig", sig));
        params.push(("format", "json".into()));
        match lastfm_post(&params) {
            Ok(json) => {
                if let Some(err) = json.get("error") {
                    tracing::warn!(?err, msg = ?json.get("message"), "Last.fm now-playing error");
                } else {
                    tracing::debug!(artist = %track.artist, title = %track.title, "Last.fm now-playing sent");
                }
            }
            Err(e) => tracing::warn!(error = %e, "Last.fm now-playing failed"),
        }
    }
    if cfg.listenbrainz_active() {
        let payload = listenbrainz_payload("playing_now", track, None);
        match listenbrainz_post(&cfg.listenbrainz_token, &payload) {
            Ok(true) => tracing::debug!(
                artist = %track.artist,
                title = %track.title,
                "ListenBrainz now-playing sent"
            ),
            Ok(false) => {}
            Err(e) => tracing::warn!(error = %e, "ListenBrainz now-playing failed"),
        }
    }
}

fn submit_scrobble(cfg: &ScrobblerConfig, track: &ScrobbleTrack, started_at: i64) {
    if cfg.lastfm_active() {
        let mut params: Vec<(&str, String)> = vec![
            ("method", "track.scrobble".into()),
            ("track[0]", track.title.clone()),
            ("artist[0]", track.artist.clone()),
            ("album[0]", track.album.clone()),
            ("timestamp[0]", started_at.to_string()),
            ("api_key", LASTFM_API_KEY.into()),
            ("sk", cfg.lastfm_session_key.clone()),
        ];
        if track.duration > 0 {
            params.push(("duration[0]", track.duration.to_string()));
        }
        let sig = lastfm_sign(&params, LASTFM_API_SECRET);
        params.push(("api_sig", sig));
        params.push(("format", "json".into()));
        match lastfm_post(&params) {
            Ok(json) => {
                if let Some(err) = json.get("error") {
                    tracing::warn!(?err, msg = ?json.get("message"), "Last.fm scrobble error");
                } else {
                    tracing::info!(artist = %track.artist, title = %track.title, "Last.fm scrobbled");
                }
            }
            Err(e) => tracing::warn!(error = %e, "Last.fm scrobble failed"),
        }
    }
    if cfg.listenbrainz_active() {
        let payload = listenbrainz_payload("single", track, Some(started_at));
        match listenbrainz_post(&cfg.listenbrainz_token, &payload) {
            Ok(true) => tracing::info!(
                artist = %track.artist,
                title = %track.title,
                "ListenBrainz scrobbled"
            ),
            Ok(false) => {}
            Err(e) => tracing::warn!(error = %e, "ListenBrainz scrobble failed"),
        }
    }
}

/// Last.fm v2 signature: concatenate every (key, value) pair sorted
/// by key (excluding `format` + `callback`), append the secret, MD5,
/// hex-lower.
fn lastfm_sign(params: &[(&str, String)], secret: &str) -> String {
    let mut keyed: Vec<&(&str, String)> = params
        .iter()
        .filter(|(k, _)| *k != "format" && *k != "callback")
        .collect();
    keyed.sort_by_key(|(k, _)| *k);
    let mut buf = String::new();
    for (k, v) in keyed {
        buf.push_str(k);
        buf.push_str(v);
    }
    buf.push_str(secret);
    let mut hasher = Md5::new();
    hasher.update(buf.as_bytes());
    let digest = hasher.finalize();
    hex_lower(&digest)
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn lastfm_post(params: &[(&str, String)]) -> Result<serde_json::Value, String> {
    let agent = ureq_agent();
    let resp = agent
        .post(LASTFM_API_URL)
        .send_form(
            &params
                .iter()
                .map(|(k, v)| (*k, v.as_str()))
                .collect::<Vec<_>>(),
        )
        .map_err(|e| e.to_string())?;
    resp.into_json::<serde_json::Value>()
        .map_err(|e| e.to_string())
}

fn listenbrainz_post(token: &str, payload: &serde_json::Value) -> Result<bool, String> {
    let agent = ureq_agent();
    let resp = agent
        .post(LISTENBRAINZ_API_URL)
        .set("Authorization", &format!("Token {token}"))
        .set("Content-Type", "application/json")
        .send_string(&payload.to_string())
        .map_err(|e| e.to_string())?;
    Ok(resp.status() == 200)
}

fn listenbrainz_payload(
    listen_type: &str,
    track: &ScrobbleTrack,
    listened_at: Option<i64>,
) -> serde_json::Value {
    let mut item = serde_json::json!({
        "track_metadata": {
            "artist_name": track.artist,
            "track_name": track.title,
            "release_name": track.album,
        }
    });
    if let Some(ts) = listened_at {
        item.as_object_mut()
            .unwrap()
            .insert("listened_at".into(), serde_json::json!(ts));
    }
    serde_json::json!({
        "listen_type": listen_type,
        "payload": [item],
    })
}

fn ureq_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
}
