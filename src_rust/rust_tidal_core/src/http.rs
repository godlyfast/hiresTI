use std::time::Duration;

use serde::de::DeserializeOwned;
use ureq::{Agent, AgentBuilder};

use crate::error::{map_ureq, RtcError, RtcResult};

/// Default user agent — kept similar to tidalapi-style ("requests" gets the
/// same TIDAL responses we know parse cleanly).
const DEFAULT_UA: &str = "TIDAL_ANDROID/2.38.0 okhttp/3.14.9";

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
