//! Settings persistence. Reads `settings.json` from the config dir and writes
//! it back atomically. The on-disk schema is owned by the existing Python
//! version and contains 50+ fields; we model only the ones the Rust UI
//! consumes today, but `extra` captures everything else so writes preserve
//! settings the rewrite hasn't reached yet.
//!
//! Forward compatibility: a field added later by Python is round-tripped via
//! `extra` and survives until we add a typed binding for it. A field removed
//! by Python stays in `extra` and gets written back — harmless, ignored.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::paths;

const FILE_NAME: &str = "settings.json";

const DEFAULT_WIDTH: i32 = 1250;
const DEFAULT_HEIGHT: i32 = 800;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_settings_version")]
    pub settings_version: i32,

    #[serde(default = "default_width")]
    pub window_width: i32,

    #[serde(default = "default_height")]
    pub window_height: i32,

    #[serde(default)]
    pub remember_window_size: bool,

    #[serde(default = "default_last_nav")]
    pub last_nav: String,

    #[serde(default = "default_last_view")]
    pub last_view: String,

    /// Catch-all for fields the rewrite hasn't typed yet. Preserved on
    /// write so existing Python-side state isn't truncated by a Rust save.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

fn default_settings_version() -> i32 {
    8
}
fn default_width() -> i32 {
    DEFAULT_WIDTH
}
fn default_height() -> i32 {
    DEFAULT_HEIGHT
}
fn default_last_nav() -> String {
    "home".into()
}
fn default_last_view() -> String {
    "grid_view".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            settings_version: default_settings_version(),
            window_width: DEFAULT_WIDTH,
            window_height: DEFAULT_HEIGHT,
            remember_window_size: false,
            last_nav: default_last_nav(),
            last_view: default_last_view(),
            extra: serde_json::Map::new(),
        }
    }
}

impl Settings {
    pub fn path() -> AppResult<PathBuf> {
        Ok(paths::config_dir()?.join(FILE_NAME))
    }

    /// Load `settings.json` from the config dir. A missing file yields
    /// defaults; a corrupt file logs a warning and falls back to defaults
    /// so a bad save can't brick the app.
    pub fn load() -> AppResult<Self> {
        let path = Self::path()?;
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(AppError::io(&path, e)),
        };
        match serde_json::from_slice::<Self>(&bytes) {
            Ok(s) => Ok(s),
            Err(e) => {
                tracing::warn!(?path, error = %e, "settings.json parse failed; using defaults");
                Ok(Self::default())
            }
        }
    }

    /// Atomic save: write to a sibling tmp file then rename, so a crash
    /// during write can never leave an empty/half-written settings.json.
    pub fn save(&self) -> AppResult<()> {
        let path = Self::path()?;
        let tmp = path.with_extension("json.tmp");
        let pretty =
            serde_json::to_vec_pretty(self).map_err(AppError::Settings)?;
        fs::write(&tmp, &pretty).map_err(|e| AppError::io(&tmp, e))?;
        fs::rename(&tmp, &path).map_err(|e| AppError::io(&path, e))?;
        Ok(())
    }
}
