//! Diagnostics dialog. Read-only dump of build info, auth state, engine
//! status, and on-disk paths so a user can copy-paste a snapshot into a
//! bug report. The contents are static — captured at open time — so
//! the dialog doesn't have to re-poll while it's visible.

use libadwaita::prelude::*;
use libadwaita::Window;
use relm4::gtk::{
    self, Box as GtkBox, HeaderBar, Orientation, ScrolledWindow, TextBuffer, TextView,
};

use crate::model::AppModel;
use crate::services::tidal_session::TidalSessionService;

pub fn present(parent: &gtk::Window, model: &AppModel, engine_state: EngineSnapshot) {
    let dialog = Window::builder()
        .modal(true)
        .transient_for(parent)
        .title("Diagnostics")
        .default_width(640)
        .default_height(540)
        .build();

    let bar = HeaderBar::new();
    let content = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .build();
    content.append(&bar);

    let body = build_body(model, &engine_state);
    let scroll = ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .child(&body)
        .build();
    content.append(&scroll);

    dialog.set_content(Some(&content));
    dialog.present();
}

/// Subset of engine state the dialog renders. Built by AppController
/// before invoking `present` so the dialog code stays UI-only.
pub struct EngineSnapshot {
    pub available: bool,
    pub last_error: Option<String>,
    pub is_playing: bool,
    pub position_seconds: f64,
    pub duration_seconds: f64,
}

fn build_body(model: &AppModel, engine: &EngineSnapshot) -> TextView {
    let mut report = String::new();
    report.push_str("=== Build ===\n");
    report.push_str(&format!(
        "hiresti_ui {}\n",
        env!("CARGO_PKG_VERSION")
    ));
    report.push_str(&format!(
        "rust_tidal_core {}\n",
        env!("CARGO_PKG_VERSION")
    ));
    report.push_str(&format!(
        "rust_audio_core {}\n",
        env!("CARGO_PKG_VERSION")
    ));
    report.push_str(&format!(
        "rust_viz_core   {}\n\n",
        env!("CARGO_PKG_VERSION")
    ));

    report.push_str("=== Auth ===\n");
    report.push_str(&format!("status: {:?}\n", model.auth.status));
    if let Some(p) = model.auth.profile.as_ref() {
        report.push_str(&format!("user_id: {}\n", p.user_id));
        report.push_str(&format!("country: {}\n", p.country_code));
        report.push_str(&format!("display: {}\n", p.display_name()));
    }
    if let Some(e) = model.auth.last_error.as_deref() {
        report.push_str(&format!("last_error: {e}\n"));
    }
    report.push('\n');

    report.push_str("=== Audio engine ===\n");
    report.push_str(&format!("available: {}\n", engine.available));
    report.push_str(&format!("is_playing: {}\n", engine.is_playing));
    report.push_str(&format!(
        "position: {:.2}s / {:.2}s\n",
        engine.position_seconds, engine.duration_seconds
    ));
    if let Some(e) = engine.last_error.as_deref() {
        report.push_str(&format!("last_error: {e}\n"));
    }
    report.push('\n');

    report.push_str("=== Playback ===\n");
    report.push_str(&format!("transport: {:?}\n", model.playback.transport));
    if let Some(t) = model.playback.current_track.as_ref() {
        report.push_str(&format!("current_track_id: {}\n", t.id));
        report.push_str(&format!("current_track_name: {}\n", t.name));
    }
    report.push_str(&format!(
        "queue_size: {} (cur {})\n\n",
        model.queue.len(),
        model.queue.current_index
    ));

    report.push_str("=== Paths ===\n");
    if let Ok(p) = TidalSessionService::token_path() {
        report.push_str(&format!("token: {}\n", p.display()));
    }
    if let Ok(p) = crate::settings::Settings::path() {
        report.push_str(&format!("settings: {}\n", p.display()));
    }
    if let Ok(p) = crate::paths::cache_dir() {
        report.push_str(&format!("cache: {}\n", p.display()));
    }
    if let Ok(p) = crate::paths::config_dir() {
        report.push_str(&format!("config: {}\n", p.display()));
    }
    report.push('\n');

    report.push_str("=== Settings (typed slice) ===\n");
    report.push_str(&format!(
        "audio.driver: {}\n",
        model.audio.driver
    ));
    report.push_str(&format!(
        "audio.device: {}\n",
        model.audio.device
    ));
    report.push_str(&format!(
        "audio.bit_perfect: {}\n",
        model.audio.bit_perfect
    ));
    report.push_str(&format!(
        "audio.exclusive_lock: {}\n",
        model.audio.exclusive_lock
    ));
    report.push_str(&format!(
        "audio.latency_profile: {}\n",
        model.audio.latency_profile
    ));
    report.push_str(&format!(
        "audio.volume: {}\n",
        model.audio.volume
    ));
    report.push_str(&format!("nav: {:?}\n", model.current_nav));

    let buffer = TextBuffer::builder().text(&report).build();
    let view = TextView::builder()
        .buffer(&buffer)
        .editable(false)
        .monospace(true)
        .top_margin(12)
        .bottom_margin(12)
        .left_margin(12)
        .right_margin(12)
        .build();
    view
}

