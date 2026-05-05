use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use ureq::Agent;

use crate::auth::{
    self, build_pkce_login_url, device_authorization, device_poll_once, pkce_exchange_code,
    refresh_access_token, DeviceLogin, PkceState, TokenInfo,
};
use crate::error::{RtcError, RtcResult};
use crate::http::{build_agent, json_body, ok_response};
use crate::request::{perform_request, RequestArgs, ResponseJson};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub user_id: i64,
    pub session_id: String,
    pub country_code: String,
    pub locale: String,
}

/// Subset of `Session` state that the Python side needs to inspect or
/// persist. Mirrors the existing hiresti_token.json schema *exactly* so old
/// token files keep loading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedToken {
    pub token_type: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    /// Stored as ISO 8601 string when present; matches the Python
    /// `datetime.isoformat()` round-trip the existing file uses.
    pub expiry_time: Option<String>,
    #[serde(default)]
    pub is_pkce: bool,
}

impl PersistedToken {
    pub fn from_token(token: &TokenInfo) -> Self {
        Self {
            token_type: Some(token.token_type.clone()),
            access_token: Some(token.access_token.clone()),
            refresh_token: token.refresh_token.clone(),
            expiry_time: token.expiry_time.map(|dt| dt.to_rfc3339()),
            is_pkce: token.is_pkce,
        }
    }

    pub fn to_token(&self) -> RtcResult<TokenInfo> {
        let access_token = self
            .access_token
            .clone()
            .ok_or_else(|| RtcError::Auth("token file missing access_token".into()))?;
        let token_type = self.token_type.clone().unwrap_or_else(|| "Bearer".into());
        let expiry_time = match &self.expiry_time {
            Some(s) if !s.is_empty() => DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
                .or_else(|| {
                    DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f%:z")
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc))
                }),
            _ => None,
        };
        Ok(TokenInfo {
            access_token,
            refresh_token: self.refresh_token.clone(),
            token_type,
            expiry_time,
            is_pkce: self.is_pkce,
        })
    }
}

pub struct Session {
    agent: Agent,
    inner: Mutex<SessionState>,
}

struct SessionState {
    token: Option<TokenInfo>,
    user: Option<UserInfo>,
    pending_pkce: Option<PkceState>,
    pending_device: Option<DeviceLogin>,
}

impl Session {
    pub fn new(pool_size: usize) -> Self {
        Self {
            agent: build_agent(pool_size),
            inner: Mutex::new(SessionState {
                token: None,
                user: None,
                pending_pkce: None,
                pending_device: None,
            }),
        }
    }

    pub fn load_token(&self, token: TokenInfo) -> RtcResult<UserInfo> {
        let user = self.fetch_user_info(&token.access_token)?;
        let mut state = self.inner.lock();
        state.token = Some(token);
        state.user = Some(user.clone());
        Ok(user)
    }

    pub fn token_snapshot(&self) -> Option<TokenInfo> {
        self.inner.lock().token.clone()
    }

    pub fn user_snapshot(&self) -> Option<UserInfo> {
        self.inner.lock().user.clone()
    }

    pub fn check_login(&self) -> bool {
        let state = self.inner.lock();
        let (Some(token), Some(user)) = (state.token.clone(), state.user.clone()) else {
            return false;
        };
        drop(state);
        let url = format!(
            "{}users/{}/subscription",
            auth::API_V1_LOCATION,
            user.user_id
        );
        let resp = ok_response(
            self.agent
                .get(&url)
                .set("Authorization", &format!("Bearer {}", token.access_token))
                .query("countryCode", &user.country_code)
                .call(),
        );
        matches!(resp, Ok(r) if (200..300).contains(&r.status()))
    }

    pub fn pkce_login_url(&self) -> RtcResult<String> {
        let state = PkceState::new();
        let url = build_pkce_login_url(&state)?;
        self.inner.lock().pending_pkce = Some(state);
        Ok(url)
    }

    pub fn pkce_finish(&self, redirect_url: &str) -> RtcResult<UserInfo> {
        let pending = self
            .inner
            .lock()
            .pending_pkce
            .take()
            .ok_or_else(|| RtcError::InvalidInput(
                "no pending PKCE login — call pkce_login_url first".into(),
            ))?;
        let token = pkce_exchange_code(&self.agent, &pending, redirect_url)?;
        self.load_token(token)
    }

    pub fn oauth_device_start(&self) -> RtcResult<DeviceLogin> {
        let login = device_authorization(&self.agent)?;
        self.inner.lock().pending_device = Some(login.clone());
        Ok(login)
    }

    /// Polls once. Returns `None` while pending, `Some(UserInfo)` on success.
    /// On expiry/error returns Err and clears the pending state.
    pub fn oauth_device_poll(&self) -> RtcResult<Option<UserInfo>> {
        let pending = self
            .inner
            .lock()
            .pending_device
            .clone()
            .ok_or_else(|| RtcError::InvalidInput(
                "no pending device login — call oauth_device_start first".into(),
            ))?;
        match device_poll_once(&self.agent, &pending) {
            Ok(Some(token)) => {
                self.inner.lock().pending_device = None;
                let user = self.load_token(token)?;
                Ok(Some(user))
            }
            Ok(None) => Ok(None),
            Err(e) => {
                self.inner.lock().pending_device = None;
                Err(e)
            }
        }
    }

    /// Generic authenticated HTTP. Available once a token is loaded.
    pub fn request(&self, args: RequestArgs) -> RtcResult<ResponseJson> {
        let (access_token, country, session_id) = {
            let state = self.inner.lock();
            let token = state
                .token
                .as_ref()
                .ok_or_else(|| RtcError::Auth("no session loaded — call load_token first".into()))?
                .clone();
            let country = state.user.as_ref().map(|u| u.country_code.clone());
            let session_id = state.user.as_ref().map(|u| u.session_id.clone());
            (token.access_token, country, session_id)
        };
        perform_request(
            &self.agent,
            &access_token,
            country.as_deref(),
            session_id.as_deref(),
            args,
        )
    }

    pub fn refresh_token(&self) -> RtcResult<TokenInfo> {
        let (refresh, is_pkce) = {
            let state = self.inner.lock();
            let token = state
                .token
                .as_ref()
                .ok_or_else(|| RtcError::Auth("no session loaded — cannot refresh".into()))?;
            let refresh = token
                .refresh_token
                .clone()
                .ok_or_else(|| RtcError::Auth("no refresh token stored".into()))?;
            (refresh, token.is_pkce)
        };
        let new_token = refresh_access_token(&self.agent, &refresh, is_pkce)?;
        let user = self.fetch_user_info(&new_token.access_token)?;
        let mut state = self.inner.lock();
        state.token = Some(new_token.clone());
        state.user = Some(user);
        Ok(new_token)
    }

    fn fetch_user_info(&self, access_token: &str) -> RtcResult<UserInfo> {
        let url = format!("{}sessions", auth::API_V1_LOCATION);
        let resp = ok_response(
            self.agent
                .get(&url)
                .set("Authorization", &format!("Bearer {}", access_token))
                .call(),
        )?;
        let json: serde_json::Value = json_body(resp)?;
        let user_id = json
            .get("userId")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| RtcError::Auth("missing userId in /sessions response".into()))?;
        let session_id = json
            .get("sessionId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RtcError::Auth("missing sessionId in /sessions response".into()))?
            .to_string();
        let country_code = json
            .get("countryCode")
            .and_then(|v| v.as_str())
            .unwrap_or("US")
            .to_string();
        Ok(UserInfo {
            user_id,
            session_id,
            country_code,
            locale: "en_US".to_string(),
        })
    }
}

/// Reads the existing hiresti_token.json schema. Tolerates the legacy nested
/// `{"data": ...}` form tidalapi writes, in case the user roundtripped via
/// `save_session_to_file`.
pub fn read_persisted_token<P: AsRef<Path>>(path: P) -> RtcResult<PersistedToken> {
    let bytes = std::fs::read(path.as_ref())?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    Ok(value_to_persisted(&value))
}

fn value_to_persisted(value: &serde_json::Value) -> PersistedToken {
    let pluck = |k: &str| -> Option<&serde_json::Value> {
        // Accept both flat ({"access_token": "..."}) and nested
        // ({"access_token": {"data": "..."}}) shapes.
        value.get(k).map(|v| match v.get("data") {
            Some(inner) => inner,
            None => v,
        })
    };
    let to_string = |v: Option<&serde_json::Value>| {
        v.and_then(|v| v.as_str()).map(str::to_string).filter(|s| !s.is_empty())
    };
    let to_bool = |v: Option<&serde_json::Value>| v.and_then(|v| v.as_bool()).unwrap_or(false);

    PersistedToken {
        token_type: to_string(pluck("token_type")),
        access_token: to_string(pluck("access_token")),
        refresh_token: to_string(pluck("refresh_token")),
        expiry_time: to_string(pluck("expiry_time")),
        is_pkce: to_bool(pluck("is_pkce")),
    }
}

pub fn write_persisted_token<P: AsRef<Path>>(
    path: P,
    persisted: &PersistedToken,
) -> RtcResult<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp: PathBuf = {
        let mut p = path.to_path_buf();
        let mut name = p.file_name().map(|n| n.to_owned()).unwrap_or_default();
        name.push(".tmp");
        p.set_file_name(name);
        p
    };
    let body = serde_json::to_vec_pretty(persisted)?;
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn persisted_token_roundtrip_flat_schema() {
        let json = br#"{
          "token_type": "Bearer",
          "access_token": "abc",
          "refresh_token": "ref",
          "expiry_time": "2026-05-05T12:00:00+00:00",
          "is_pkce": true
        }"#;
        let v: serde_json::Value = serde_json::from_slice(json).unwrap();
        let p = super::value_to_persisted(&v);
        assert_eq!(p.access_token.as_deref(), Some("abc"));
        assert_eq!(p.refresh_token.as_deref(), Some("ref"));
        assert!(p.is_pkce);
        let token = p.to_token().unwrap();
        assert_eq!(token.access_token, "abc");
        assert!(token.is_pkce);
        assert!(token.expiry_time.is_some());
    }

    #[test]
    fn persisted_token_roundtrip_nested_schema() {
        let json = br#"{
          "token_type": {"data": "Bearer"},
          "access_token": {"data": "abc"},
          "refresh_token": {"data": "ref"},
          "is_pkce": {"data": false}
        }"#;
        let v: serde_json::Value = serde_json::from_slice(json).unwrap();
        let p = super::value_to_persisted(&v);
        assert_eq!(p.access_token.as_deref(), Some("abc"));
        assert!(!p.is_pkce);
    }

    #[test]
    fn write_then_read_atomic_replace() {
        let dir = tempdir();
        let path = dir.join("hiresti_token.json");
        let token = TokenInfo {
            access_token: "tok".into(),
            refresh_token: Some("ref".into()),
            token_type: "Bearer".into(),
            expiry_time: Some(Utc::now()),
            is_pkce: true,
        };
        let persisted = PersistedToken::from_token(&token);
        write_persisted_token(&path, &persisted).unwrap();
        let back = read_persisted_token(&path).unwrap();
        assert_eq!(back.access_token, persisted.access_token);
        assert_eq!(back.is_pkce, persisted.is_pkce);
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rtc_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::File::create(dir.join(".keep")).map(|mut f| f.write_all(b""));
        dir
    }
}
