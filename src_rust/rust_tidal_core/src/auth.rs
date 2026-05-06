//! TIDAL OAuth and PKCE authentication.
//!
//! Constants here mirror tidalapi's runtime values. They aren't secrets —
//! they're the public client identifiers TIDAL's official Android app uses,
//! reverse-engineered by the tidalapi project. The crucial property is that
//! the PKCE pair (`CLIENT_ID_PKCE` / `CLIENT_SECRET_PKCE`) returns FLAC and
//! Hi-Res Lossless streams from `playbackinfopostpaywall`, while the legacy
//! device-code pair caps at 320 kbps AAC.

use base64::{engine::general_purpose::STANDARD, engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use ureq::Agent;
use url::Url;

use crate::error::{RtcError, RtcResult};
use crate::http::{json_body, ok_response};

pub const CLIENT_ID: &str = "fX2JxdmntZWK0ixT";
pub const CLIENT_SECRET: &str = "1Nn9AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg=";
pub const CLIENT_ID_PKCE: &str = "6BDSRdpK9hqEBTgU";
pub const CLIENT_SECRET_PKCE: &str = "xeuPmY7nbpZ9IIbLAcQ93shka1VNheUAqN6IcszjTG8=";

pub const API_OAUTH_TOKEN: &str = "https://auth.tidal.com/v1/oauth2/token";
pub const API_DEVICE_AUTH: &str = "https://auth.tidal.com/v1/oauth2/device_authorization";
pub const API_PKCE_AUTH: &str = "https://login.tidal.com/authorize";
pub const PKCE_REDIRECT_URI: &str = "https://tidal.com/android/login/auth";
pub const API_V1_LOCATION: &str = "https://api.tidal.com/v1/";
#[allow(dead_code)] // wired up in Phase 2 (HTTP/RPC + page.get)
pub const API_V2_LOCATION: &str = "https://api.tidal.com/v2/";

pub const SCOPE_DEVICE: &str = "r_usr w_usr w_sub";
pub const SCOPE_PKCE: &str = "r_usr+w_usr+w_sub";

/// State carried between `pkce_login_url()` and `pkce_exchange_code()`. The
/// verifier and unique key are generated once at URL construction and reused
/// on the token exchange — losing them mid-flow forces a fresh login URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PkceState {
    pub code_verifier: String,
    pub code_challenge: String,
    pub client_unique_key: String,
}

impl PkceState {
    pub fn new() -> Self {
        let mut buf = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut buf);
        let code_verifier = URL_SAFE_NO_PAD.encode(&buf);

        let challenge_digest = Sha256::digest(code_verifier.as_bytes());
        let code_challenge = URL_SAFE_NO_PAD.encode(challenge_digest);

        let mut key_bytes = [0u8; 8];
        rand::thread_rng().fill_bytes(&mut key_bytes);
        let client_unique_key = format!("{:016x}", u64::from_be_bytes(key_bytes));

        PkceState {
            code_verifier,
            code_challenge,
            client_unique_key,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: String,
    /// UTC; serialized as ISO 8601 to match Python json roundtrip behavior.
    pub expiry_time: Option<DateTime<Utc>>,
    pub is_pkce: bool,
}

impl TokenInfo {
    pub fn from_oauth_response(json: &serde_json::Value, is_pkce: bool) -> RtcResult<Self> {
        let access_token = json
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RtcError::Auth("missing access_token in token response".into()))?
            .to_string();
        let refresh_token = json
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let token_type = json
            .get("token_type")
            .and_then(|v| v.as_str())
            .unwrap_or("Bearer")
            .to_string();
        let expires_in = json.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(0);
        let expiry_time = if expires_in > 0 {
            Some(Utc::now() + ChronoDuration::seconds(expires_in))
        } else {
            None
        };
        Ok(Self {
            access_token,
            refresh_token,
            token_type,
            expiry_time,
            is_pkce,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceLogin {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: i64,
    pub interval: i64,
}

pub fn build_pkce_login_url(state: &PkceState) -> RtcResult<String> {
    let mut url = Url::parse(API_PKCE_AUTH)?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", PKCE_REDIRECT_URI)
        .append_pair("client_id", CLIENT_ID_PKCE)
        .append_pair("lang", "EN")
        .append_pair("appMode", "android")
        .append_pair("client_unique_key", &state.client_unique_key)
        .append_pair("code_challenge", &state.code_challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("restrict_signup", "true");
    Ok(url.to_string())
}

pub fn extract_pkce_code(redirect_url: &str) -> RtcResult<String> {
    let url = Url::parse(redirect_url).map_err(|e| {
        RtcError::InvalidInput(format!("redirect URL not parseable: {} ({})", redirect_url, e))
    })?;
    for (k, v) in url.query_pairs() {
        if k == "code" {
            return Ok(v.into_owned());
        }
    }
    Err(RtcError::InvalidInput(
        "redirect URL has no `code` query parameter".into(),
    ))
}

pub fn pkce_exchange_code(
    agent: &Agent,
    state: &PkceState,
    redirect_url: &str,
) -> RtcResult<TokenInfo> {
    let code = extract_pkce_code(redirect_url)?;
    let form: HashMap<&str, &str> = HashMap::from([
        ("code", code.as_str()),
        ("client_id", CLIENT_ID_PKCE),
        ("grant_type", "authorization_code"),
        ("redirect_uri", PKCE_REDIRECT_URI),
        ("scope", SCOPE_PKCE),
        ("code_verifier", state.code_verifier.as_str()),
        ("client_unique_key", state.client_unique_key.as_str()),
    ]);
    let resp = ok_response(agent.post(API_OAUTH_TOKEN).send_form(&form_pairs(&form)))?;
    let json: serde_json::Value = json_body(resp)?;
    TokenInfo::from_oauth_response(&json, true)
}

pub fn device_authorization(agent: &Agent) -> RtcResult<DeviceLogin> {
    let form: HashMap<&str, &str> = HashMap::from([
        ("client_id", CLIENT_ID),
        ("scope", SCOPE_DEVICE),
    ]);
    let resp = ok_response(agent.post(API_DEVICE_AUTH).send_form(&form_pairs(&form)))?;
    let json: serde_json::Value = json_body(resp)?;
    let user_code = json
        .get("userCode")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RtcError::Auth("missing userCode in device authorization response".into()))?
        .to_string();
    let device_code = json
        .get("deviceCode")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RtcError::Auth("missing deviceCode in device authorization response".into()))?
        .to_string();
    // Tidal's device-authorization response returns these as bare
    // host paths (e.g. `link.tidal.com/PCLMJ`) — without `https://`
    // xdg-open / gio resolve them as relative file paths. Always
    // normalize to a full URL so downstream consumers can hand them
    // straight to a browser opener.
    let verification_uri = ensure_https(
        json.get("verificationUri")
            .and_then(|v| v.as_str())
            .unwrap_or("link.tidal.com"),
    );
    let verification_uri_complete = ensure_https(
        &json
            .get("verificationUriComplete")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| format!("link.tidal.com/{user_code}")),
    );
    let expires_in = json.get("expiresIn").and_then(|v| v.as_i64()).unwrap_or(300);
    let interval = json.get("interval").and_then(|v| v.as_i64()).unwrap_or(2);
    Ok(DeviceLogin {
        device_code,
        user_code,
        verification_uri,
        verification_uri_complete,
        expires_in,
        interval,
    })
}

/// Single device-code poll. Returns `Ok(Some(token))` on success, `Ok(None)`
/// while still pending, or a non-recoverable error otherwise. The expired
/// case is signaled by ureq returning Status(400) with body `error=expired_token`.
pub fn device_poll_once(agent: &Agent, login: &DeviceLogin) -> RtcResult<Option<TokenInfo>> {
    let form: HashMap<&str, &str> = HashMap::from([
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("device_code", login.device_code.as_str()),
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ("scope", SCOPE_DEVICE),
    ]);
    let call = agent.post(API_OAUTH_TOKEN).send_form(&form_pairs(&form));
    match call {
        Ok(resp) => {
            let json: serde_json::Value = json_body(resp)?;
            Ok(Some(TokenInfo::from_oauth_response(&json, false)?))
        }
        Err(ureq::Error::Status(status, response)) if status == 400 || status == 401 => {
            let body = response.into_string().unwrap_or_default();
            // tidalapi convention: parse the JSON error key.
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body) {
                let err_code = parsed.get("error").and_then(|v| v.as_str()).unwrap_or("");
                match err_code {
                    "authorization_pending" | "slow_down" => Ok(None),
                    "expired_token" => Err(RtcError::Auth("device login link expired".into())),
                    other => Err(RtcError::Auth(format!(
                        "device poll error: {} ({})",
                        other, body
                    ))),
                }
            } else {
                Err(RtcError::Auth(format!("device poll {}: {}", status, body)))
            }
        }
        Err(other) => Err(crate::error::map_ureq(other)),
    }
}

pub fn refresh_access_token(
    agent: &Agent,
    refresh_token: &str,
    is_pkce: bool,
) -> RtcResult<TokenInfo> {
    let (cid, csecret) = if is_pkce {
        (CLIENT_ID_PKCE, CLIENT_SECRET_PKCE)
    } else {
        (CLIENT_ID, CLIENT_SECRET)
    };
    let form: HashMap<&str, &str> = HashMap::from([
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", cid),
        ("client_secret", csecret),
    ]);
    let resp = ok_response(agent.post(API_OAUTH_TOKEN).send_form(&form_pairs(&form)))?;
    let json: serde_json::Value = json_body(resp)?;
    let mut info = TokenInfo::from_oauth_response(&json, is_pkce)?;
    if info.refresh_token.is_none() {
        info.refresh_token = Some(refresh_token.to_string());
    }
    Ok(info)
}

fn form_pairs<'a>(map: &'a HashMap<&'a str, &'a str>) -> Vec<(&'a str, &'a str)> {
    map.iter().map(|(k, v)| (*k, *v)).collect()
}

/// Prepend `https://` if the URI doesn't already carry an http(s) scheme.
/// Tidal's device-auth response returns bare host paths like
/// `link.tidal.com/USERCODE`; without normalization, xdg-open / gio
/// resolve those as relative file paths and the browser open fails.
fn ensure_https(uri: &str) -> String {
    let trimmed = uri.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    }
}

/// Re-encodes a UTF-8 string to a `Authorization: Basic ...` header value.
/// Currently unused — kept here for the eventual openapi flow which uses
/// HTTP basic auth on top of client_id/client_secret.
#[allow(dead_code)]
pub fn basic_auth_header(client_id: &str, client_secret: &str) -> String {
    let pair = format!("{}:{}", client_id, client_secret);
    format!("Basic {}", STANDARD.encode(pair.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_state_produces_valid_challenge() {
        let s = PkceState::new();
        assert_eq!(s.code_verifier.len(), 43); // 32 bytes urlsafe-no-pad
        assert_eq!(s.code_challenge.len(), 43); // sha256 = 32 bytes
        assert_eq!(s.client_unique_key.len(), 16);
    }

    #[test]
    fn pkce_login_url_has_required_params() {
        let s = PkceState::new();
        let url = build_pkce_login_url(&s).unwrap();
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=6BDSRdpK9hqEBTgU"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains(&format!("code_challenge={}", s.code_challenge)));
    }

    #[test]
    fn extract_code_from_redirect() {
        let url = "https://tidal.com/android/login/auth?code=abc123&state=foo";
        assert_eq!(extract_pkce_code(url).unwrap(), "abc123");
    }

    #[test]
    fn extract_code_missing_returns_invalid_input() {
        let url = "https://tidal.com/android/login/auth?state=foo";
        let err = extract_pkce_code(url).unwrap_err();
        assert!(matches!(err, RtcError::InvalidInput(_)));
    }
}
