//! Shared building blocks used by every library view.
//!
//! - `ViewLoadState`: idle / loading / loaded / failed lifecycle that
//!   each view's update() drives.
//! - `LibraryViewOutput`: the unified navigation/playback output enum
//!   library views emit. The root forwards into AppInput.
//! - Helpers for the standard placeholder widgets (loading spinner,
//!   error label, empty-state label) so views don't reinvent the
//!   styling.

use relm4::gtk::{self, prelude::*, Box as GtkBox, Label, Orientation, Spinner};

#[derive(Debug, Clone, Default)]
pub enum ViewLoadState {
    #[default]
    Idle,
    Loading,
    Loaded,
    Failed(String),
}

impl ViewLoadState {
    /// Convenience predicates kept here for Phase 6 callers (Discovery
    /// pages reuse this enum and want symmetric is-state checks).
    #[allow(dead_code)]
    pub fn is_loaded(&self) -> bool {
        matches!(self, Self::Loaded)
    }
    #[allow(dead_code)]
    pub fn is_loading(&self) -> bool {
        matches!(self, Self::Loading)
    }
}

/// Output every library view emits. Variants name the kind of detail
/// page the user wants to open; the root component is responsible for
/// dispatching to the appropriate detail-view component (Phase 7).
#[derive(Debug, Clone)]
#[allow(dead_code)] // OpenArtist/OpenMix/OpenPlaylist/PlayTrack land in Phase 7
pub enum LibraryViewOutput {
    OpenAlbum { id: String, title: String },
    OpenArtist { id: String, name: String },
    OpenPlaylist { uuid: String, title: String },
    OpenMix { id: String, title: String },
    PlayTrack { track_id: i64 },
}

/// Centered spinner for the Loading state. Same shape across every view.
pub fn build_loading_widget() -> GtkBox {
    let wrap = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .vexpand(true)
        .hexpand(true)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .spacing(8)
        .build();
    let spinner = Spinner::builder()
        .spinning(true)
        .width_request(32)
        .height_request(32)
        .build();
    let label = Label::builder()
        .label("Loading…")
        .css_classes(["dim-label"])
        .build();
    wrap.append(&spinner);
    wrap.append(&label);
    wrap
}

/// Centered "no items" placeholder — used when a successful fetch
/// returned an empty list (e.g. user has no favorite albums).
pub fn build_empty_widget(message: &str) -> GtkBox {
    let wrap = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .vexpand(true)
        .hexpand(true)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .spacing(8)
        .build();
    let label = Label::builder()
        .label(message)
        .css_classes(["dim-label"])
        .build();
    wrap.append(&label);
    wrap
}

/// Centered error message + (later) retry button.
pub fn build_error_widget(message: &str) -> GtkBox {
    let wrap = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .vexpand(true)
        .hexpand(true)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .spacing(8)
        .margin_start(24)
        .margin_end(24)
        .build();
    let label = Label::builder()
        .label(format!("Couldn't load:\n{message}"))
        .wrap(true)
        .justify(gtk::Justification::Center)
        .css_classes(["error"])
        .build();
    wrap.append(&label);
    wrap
}
