//! Domain state types for the Rust UI rewrite.
//!
//! Each submodule owns one concern; the root `AppModel` (in `model.rs`)
//! composes them. The split mirrors the natural fault lines we found in
//! the Python codebase, with the explicit goal of avoiding the god-class
//! / shared-attribute style that made the Python version hard to extend:
//! every Relm4 component will receive only the sub-state it actually needs.
//!
//! - `auth`     — login session + the logged-in user profile
//! - `library`  — user-owned data (favorite albums/tracks/artists/playlists/mixes)
//! - `playback` — current track, position, queue, play mode, volume
//! - `nav`      — active top-level nav target + view stack history
//! - `search`   — current query, results, recent searches
//! - `history`  — local play history (independent of TIDAL favorites)
//! - `audio`    — driver / device / latency / DSP chain config
//! - `viz`      — visualizer effect / theme / bar count

// Phase 2 defines the full state vocabulary; many fields and helpers
// stay unused until the Phase 3+ components wire them. Suppress
// dead-code warnings on each submodule so the build stays quiet —
// once a field has a real reader the lint cost goes away naturally.
#[allow(dead_code)]
pub mod audio;
#[allow(dead_code)]
pub mod auth;
#[allow(dead_code)]
pub mod history;
#[allow(dead_code)]
pub mod library;
#[allow(dead_code)]
pub mod nav;
#[allow(dead_code)]
pub mod playback;
#[allow(dead_code)]
pub mod search;
#[allow(dead_code)]
pub mod viz;
