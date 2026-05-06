//! DSP preset save/load.
//!
//! Snapshots the `dsp_*` keys out of `Settings::extra` into named JSON
//! files under `~/.config/hiresti/dsp_presets/<name>.json`. Loading
//! reads one of those files and overlays its keys back onto a Settings
//! clone — non-DSP fields (window size, last_nav, etc.) survive.
//!
//! No DSP UI panel ships in Phase 8 yet; the preset machinery is
//! useful on its own because the user can edit `dsp_*` values in
//! `settings.json` directly, snapshot them, and toggle between named
//! configurations without re-editing the file.

use std::fs;
use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::error::{AppError, AppResult};
use crate::paths;
use crate::settings::Settings;

const PRESETS_DIR: &str = "dsp_presets";

/// Filter predicate for "is this settings key part of a DSP preset?".
/// We snapshot anything that starts with `dsp_` so additions to the
/// DSP graph (Phase 9 may add e.g. `dsp_spectrum_*`) are picked up
/// without code changes here.
fn is_dsp_key(k: &str) -> bool {
    k.starts_with("dsp_")
}

pub fn presets_dir() -> AppResult<PathBuf> {
    let mut p = paths::config_dir()?;
    p.push(PRESETS_DIR);
    if !p.exists() {
        fs::create_dir_all(&p)
            .map_err(|e| AppError::ConfigDir(format!("create dsp_presets dir: {e}")))?;
    }
    Ok(p)
}

fn preset_path(name: &str) -> AppResult<PathBuf> {
    let safe: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == ' ')
        .collect::<String>()
        .trim()
        .replace(' ', "_");
    if safe.is_empty() {
        return Err(AppError::ConfigDir(
            "preset name cannot be empty after sanitization".into(),
        ));
    }
    let mut p = presets_dir()?;
    p.push(format!("{safe}.json"));
    Ok(p)
}

/// List preset names in alphabetical order. Missing dir is treated as
/// "no presets" rather than an error.
pub fn list_presets() -> AppResult<Vec<String>> {
    let dir = presets_dir()?;
    let mut out: Vec<String> = Vec::new();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => {
            return Err(AppError::ConfigDir(format!(
                "read dsp_presets dir: {e}"
            )))
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                out.push(stem.to_string());
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Snapshot the DSP-prefixed keys in `settings.extra` into a named
/// preset file. Atomic write (tmp + rename).
pub fn save_preset(name: &str, settings: &Settings) -> AppResult<PathBuf> {
    let dsp_only: Map<String, Value> = settings
        .extra
        .iter()
        .filter(|(k, _)| is_dsp_key(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if dsp_only.is_empty() {
        return Err(AppError::ConfigDir(
            "no dsp_* settings found to snapshot".into(),
        ));
    }
    let path = preset_path(name)?;
    let pretty = serde_json::to_vec_pretty(&dsp_only).map_err(AppError::Settings)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, &pretty).map_err(|e| AppError::io(&tmp, e))?;
    fs::rename(&tmp, &path).map_err(|e| AppError::io(&path, e))?;
    Ok(path)
}

/// Read a preset file into a JSON map of dsp_* keys.
pub fn load_preset(name: &str) -> AppResult<Map<String, Value>> {
    let path = preset_path(name)?;
    let bytes = fs::read(&path).map_err(|e| AppError::io(&path, e))?;
    let parsed: Value = serde_json::from_slice(&bytes).map_err(AppError::Settings)?;
    let map = parsed.as_object().cloned().unwrap_or_default();
    Ok(map)
}

/// Overlay the DSP-only fields from `preset` onto a Settings clone.
/// Non-DSP keys in `preset` are ignored defensively (a hand-edited
/// preset file shouldn't be able to clobber window_width etc.).
pub fn apply_preset(base: &Settings, preset: &Map<String, Value>) -> Settings {
    let mut out = base.clone();
    // First clear all dsp_* keys so a preset that drops a key (e.g.
    // disables convolver and removes its config) doesn't leave the
    // old values lingering.
    out.extra.retain(|k, _| !is_dsp_key(k));
    for (k, v) in preset.iter().filter(|(k, _)| is_dsp_key(k)) {
        out.extra.insert(k.clone(), v.clone());
    }
    out
}

pub fn delete_preset(name: &str) -> AppResult<()> {
    let path = preset_path(name)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(AppError::io(&path, e)),
    }
}
