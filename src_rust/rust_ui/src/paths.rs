//! Config + cache directory resolution. Mirrors the Python
//! `utils/paths.py` so existing on-disk state (settings.json,
//! hiresti_token.json, profiles/, covers/) is found unchanged after
//! the rewrite — users keep their tokens, history, and cached art.

use std::env;
use std::path::PathBuf;

use crate::error::{AppError, AppResult};

const APP_DIR: &str = "hiresti";

/// `~/.config/hiresti` (or `$XDG_CONFIG_HOME/hiresti`).
pub fn config_dir() -> AppResult<PathBuf> {
    if let Ok(raw) = env::var("XDG_CONFIG_HOME") {
        if !raw.trim().is_empty() {
            return Ok(PathBuf::from(raw).join(APP_DIR));
        }
    }
    let home = home_dir()?;
    Ok(home.join(".config").join(APP_DIR))
}

/// `~/.cache/hiresti` (or `$XDG_CACHE_HOME/hiresti`).
pub fn cache_dir() -> AppResult<PathBuf> {
    if let Ok(raw) = env::var("XDG_CACHE_HOME") {
        if !raw.trim().is_empty() {
            return Ok(PathBuf::from(raw).join(APP_DIR));
        }
    }
    let home = home_dir()?;
    Ok(home.join(".cache").join(APP_DIR))
}

/// `~/.local/share/hiresti` (or `$XDG_DATA_HOME/hiresti`).
/// Used in Phase 5+ for the local SQLite store; defined now so the
/// path module is feature-complete from the start.
#[allow(dead_code)]
pub fn data_dir() -> AppResult<PathBuf> {
    if let Ok(raw) = env::var("XDG_DATA_HOME") {
        if !raw.trim().is_empty() {
            return Ok(PathBuf::from(raw).join(APP_DIR));
        }
    }
    let home = home_dir()?;
    Ok(home.join(".local").join("share").join(APP_DIR))
}

fn home_dir() -> AppResult<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| AppError::ConfigDir("HOME not set".into()))
}

pub fn ensure_dirs() -> AppResult<()> {
    let cfg = config_dir()?;
    std::fs::create_dir_all(&cfg).map_err(|e| AppError::io(&cfg, e))?;
    let cache = cache_dir()?;
    std::fs::create_dir_all(&cache).map_err(|e| AppError::io(&cache, e))?;
    Ok(())
}
