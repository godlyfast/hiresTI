//! Login session state.
//!
//! Mirrors the shape of the Python `_UserView` (added in commit a0ef6b71)
//! plus the lifecycle bits that the Python `TidalBackend` carried as
//! ad-hoc attributes (`_last_login_error`, `_pending_oauth_thread`).

use rust_tidal_core::api::UserInfo;

#[derive(Debug, Clone, Default)]
pub struct UserProfile {
    pub user_id: i64,
    pub first_name: String,
    pub last_name: String,
    pub username: String,
    pub email: String,
    /// Free-form display name from `users/{uid}/profileMetadata.name`,
    /// when present. Falls back to `first_name + last_name` then
    /// `username` for UI display (see `display_name`).
    pub display_name: Option<String>,
    /// `country_code` and `locale` from `UserInfo`. Used for some
    /// region-conditional API calls.
    pub country_code: String,
    pub locale: String,
}

impl UserProfile {
    pub fn from_user_info(info: &UserInfo) -> Self {
        Self {
            user_id: info.user_id,
            country_code: info.country_code.clone(),
            locale: info.locale.clone(),
            ..Self::default()
        }
    }

    pub fn display_name(&self) -> &str {
        if let Some(n) = self.display_name.as_deref() {
            if !n.is_empty() {
                return n;
            }
        }
        if !self.first_name.is_empty() {
            return &self.first_name;
        }
        if !self.username.is_empty() {
            return &self.username;
        }
        "User"
    }

    /// Storage scope key, e.g. `u_204980945`. Used to namespace local
    /// history / playlist stores per account; matches the Python
    /// `app_storage_scope._account_scope_from_backend_user` output.
    pub fn storage_scope(&self) -> String {
        if self.user_id <= 0 {
            "guest".into()
        } else {
            format!("u_{}", self.user_id)
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AuthState {
    pub status: AuthStatus,
    pub profile: Option<UserProfile>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AuthStatus {
    #[default]
    LoggedOut,
    /// PKCE / device-code OAuth flow in progress. The Phase-4 login
    /// component owns the actual flow state; this enum just signals
    /// the UI's coarse state.
    Authenticating,
    LoggedIn,
}

impl AuthState {
    pub fn is_logged_in(&self) -> bool {
        matches!(self.status, AuthStatus::LoggedIn) && self.profile.is_some()
    }

    pub fn storage_scope(&self) -> String {
        self.profile
            .as_ref()
            .map(UserProfile::storage_scope)
            .unwrap_or_else(|| "guest".into())
    }
}
