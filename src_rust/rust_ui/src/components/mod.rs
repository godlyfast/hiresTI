//! Top-level Relm4 components composing the main window.
//!
//! Each region of the UI is a self-contained component owning its own
//! state slice and exposing an `Output` enum that the root forwards back
//! into `AppInput`. This keeps the component tree narrow at every level
//! and matches Relm4's recommended pattern: parents pass init data down,
//! children emit output up; no shared mutable refs.

pub mod about_dialog;
pub mod content_stack;
pub mod diagnostics_dialog;
pub mod dsp_preset_dialog;
pub mod header;
pub mod login_dialog;
pub mod mini_player;
pub mod settings_dialog;
pub mod sidebar;
pub mod signal_path_window;
pub mod views;
