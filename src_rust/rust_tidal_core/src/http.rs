use std::time::Duration;

use serde::de::DeserializeOwned;
use ureq::{Agent, AgentBuilder};

use crate::error::{map_ureq, RtcError, RtcResult};

/// User agent matched to tidalapi 2025.7.16 — TIDAL endpoints are picky and
/// some return 400/403 with the wrong UA, so keeping parity avoids drift.
pub const DEFAULT_UA: &str = "Mozilla/5.0 (Linux; Android 12; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/91.0.4472.114 Safari/537.36";

/// Tidal's `x-tidal-client-version` header tracks the client identity. tidalapi
/// pins "2025.7.16"; we mirror it for compatibility with whatever endpoint
/// validation is in place upstream.
pub const TIDAL_CLIENT_VERSION: &str = "2025.7.16";

/// Default API limit param tidalapi sends on every request. Some endpoints
/// (e.g. home/feed/static) appear to validate its presence even though the
/// docs don't require it.
pub const DEFAULT_ITEM_LIMIT: i64 = 1000;

pub fn build_agent(pool_size: usize) -> Agent {
    let pool = pool_size.max(8);
    AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .max_idle_connections(pool)
        .max_idle_connections_per_host(pool)
        .user_agent(DEFAULT_UA)
        .build()
}

pub fn json_body<T: DeserializeOwned>(response: ureq::Response) -> RtcResult<T> {
    let status = response.status();
    let text = response
        .into_string()
        .map_err(|e| RtcError::Network(e.to_string()))?;
    if !(200..300).contains(&status) {
        return Err(if (500..600).contains(&status) {
            RtcError::Server { status, body: text }
        } else if status == 404 {
            RtcError::NotFound(text)
        } else if matches!(status, 401 | 403) {
            RtcError::Auth(format!("HTTP {}: {}", status, text))
        } else {
            RtcError::Client { status, body: text }
        });
    }
    if text.trim().is_empty() {
        return Err(RtcError::Other("empty response body".into()));
    }
    serde_json::from_str(&text).map_err(RtcError::Json)
}

pub fn ok_response(call: Result<ureq::Response, ureq::Error>) -> RtcResult<ureq::Response> {
    call.map_err(map_ureq)
}
