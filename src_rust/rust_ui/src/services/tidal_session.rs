//! Wraps `rust_tidal_core::api::Session` with the threading + persistence
//! shims the UI needs. Session methods block on HTTP so every operation
//! that touches the network spawns a dedicated thread; results are
//! delivered back to the UI via the Relm4 sender supplied by the caller.
//!
//! The Session itself is wrapped in `Arc` so multiple in-flight worker
//! threads (token refresh + active fetch + login poll) can hold it
//! simultaneously. `Session` already has internal locking for its
//! mutable state, so an Arc is enough.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use rust_tidal_core::api::{
    read_persisted_token, write_persisted_token, DeviceLogin, PersistedToken, RequestArgs,
    RtcError, Session, UserInfo,
};

use crate::error::{AppError, AppResult};
use crate::paths;

const TOKEN_FILE: &str = "hiresti_token.json";
/// HTTP connection pool size. Matches the Python default (`HIRESTI_HTTP_POOL_SIZE`).
const POOL_SIZE: usize = 64;

#[derive(Clone)]
pub struct TidalSessionService {
    session: Arc<Session>,
}

impl TidalSessionService {
    pub fn new() -> Self {
        Self {
            session: Arc::new(Session::new(POOL_SIZE)),
        }
    }

    /// Reserved for Phase 5+ when data-fetch workers need a direct
    /// Session handle. Allow dead until then.
    #[allow(dead_code)]
    pub fn handle(&self) -> Arc<Session> {
        Arc::clone(&self.session)
    }

    pub fn token_path() -> AppResult<PathBuf> {
        Ok(paths::config_dir()?.join(TOKEN_FILE))
    }

    /// Read the persisted token from disk if present. Returns `Ok(None)`
    /// when the file simply doesn't exist (first run / logged out).
    pub fn load_persisted() -> AppResult<Option<PersistedToken>> {
        let path = Self::token_path()?;
        if !path.exists() {
            return Ok(None);
        }
        match read_persisted_token(&path) {
            Ok(t) => Ok(Some(t)),
            Err(e) => {
                tracing::warn!(?path, error = %e, "token file present but unreadable");
                Ok(None)
            }
        }
    }

    /// Save the current Session's token to disk atomically. Match the
    /// Python `save_session()` behavior: we round-trip via
    /// `token_snapshot` → `PersistedToken::from_token` so the file
    /// format stays identical across both implementations.
    pub fn save_persisted(&self) -> AppResult<()> {
        let token = self
            .session
            .token_snapshot()
            .ok_or_else(|| AppError::ConfigDir("no token loaded".into()))?;
        let snapshot = PersistedToken::from_token(&token);
        let path = Self::token_path()?;
        write_persisted_token(&path, &snapshot)
            .map_err(|e| AppError::ConfigDir(format!("write token: {e}")))?;
        Ok(())
    }

    /// Restore from a previously-loaded token. The session re-validates
    /// the token against /v1/sessions; returns the resulting `UserInfo`
    /// on success. Cold-start path goes through
    /// `load_token_with_refresh_blocking` instead, which auto-refreshes
    /// on auth-error; this raw variant is here for callers that want to
    /// surface the auth error directly (e.g. a Phase 8 settings dialog).
    #[allow(dead_code)]
    pub fn load_token_blocking(
        &self,
        token: PersistedToken,
    ) -> Result<UserInfo, RtcError> {
        let info = self.session.load_token(token.to_token()?)?;
        Ok(info)
    }

    /// Cold-start path: stage the saved token in the session, validate
    /// against /v1/sessions, and refresh in-place if the access token
    /// has expired. Mirrors what the Python `_restore_session_from_saved_data`
    /// did via the recovery loop, but as a single atomic call inside
    /// rust_tidal_core.
    pub fn load_token_with_refresh_blocking(
        &self,
        token: PersistedToken,
    ) -> Result<UserInfo, RtcError> {
        self.session.restore_with_refresh(token.to_token()?)
    }

    /// Cheap "is this access token still valid" probe — single GET to
    /// the sessions endpoint. Blocks; spawn off-thread before calling.
    pub fn check_login_blocking(&self) -> bool {
        self.session.check_login()
    }

    /// Kick off a device-code OAuth flow. Returns the `DeviceLogin`
    /// payload (verification URL + user code + polling interval) which
    /// the UI shows to the user.
    pub fn oauth_device_start_blocking(&self) -> Result<DeviceLogin, RtcError> {
        self.session.oauth_device_start()
    }

    /// Poll the device-code endpoint once. Returns `Ok(Some(user))` once
    /// the user authorizes, `Ok(None)` while still pending. Errors are
    /// terminal (token expired, denied, network).
    pub fn oauth_device_poll_blocking(&self) -> Result<Option<UserInfo>, RtcError> {
        self.session.oauth_device_poll()
    }

    /// Read the user-profile fields from `/v1/users/{id}` and merge with
    /// the `UserInfo` we already have. Mirrors the Python
    /// `_build_user_view` helper introduced in commit a0ef6b71.
    pub fn fetch_profile_blocking(
        &self,
        user_id: i64,
    ) -> Result<serde_json::Value, RtcError> {
        let resp = self.session.request(RequestArgs {
            method: "GET".into(),
            path: format!("users/{user_id}"),
            base_url: None,
            params: None,
            headers: None,
            json_body: None,
            form_body: false,
        })?;
        if resp.ok {
            Ok(resp.body)
        } else {
            Err(RtcError::Other(format!(
                "users/{user_id} returned status {}: {}",
                resp.status, resp.body
            )))
        }
    }
}

/// Helper: spawn `f` on a fresh thread and call `cb` with its result.
/// The callback is invoked on the worker thread; senders that target
/// the GTK main thread (Relm4 `ComponentSender::input_sender()`) marshal
/// across threads safely.
pub fn spawn_blocking<F, T>(f: F, cb: impl FnOnce(T) + Send + 'static)
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    thread::spawn(move || {
        let out = f();
        cb(out);
    });
}
