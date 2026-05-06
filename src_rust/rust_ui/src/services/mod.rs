//! Long-lived services that the UI components consume.
//!
//! Each service wraps an underlying Rust crate (rust_tidal_core,
//! rust_audio_core, rust_viz_core) with a Rust-typed API and any
//! threading shims the UI needs (blocking HTTP calls dispatched to a
//! worker thread, etc.).

pub mod covers;
pub mod dsp_preset;
pub mod lyrics;
pub mod mpris;
pub mod scrobbler;
pub mod tidal_session;
pub mod tray;
