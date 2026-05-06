//! Local play history (independent of TIDAL favorites).
//!
//! Backed by `~/.cache/hiresti/profiles/<scope>/history.json`. The Python
//! HistoryManager (`src/models/history.py`) owns the on-disk schema; we
//! mirror it for forward compatibility so the rewrite picks up existing
//! user history transparently.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HistoryTrack {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Cover UUID — same format the TIDAL `Track.album.cover` field uses.
    pub cover: Option<String>,
    pub duration: u32,
    /// Unix timestamp (seconds) when this entry was added.
    pub played_at: i64,
}

#[derive(Debug, Clone, Default)]
pub struct LocalHistoryState {
    /// Recently played tracks, most recent first. Capped via the
    /// `HistoryManager.MAX_TRACKS` constant on disk; we don't enforce
    /// the cap ourselves on read — trust the writer.
    pub tracks: Vec<HistoryTrack>,
    /// Recently visited album/artist/playlist surfaces. Used by the
    /// "Recent" sidebar section.
    pub recent_albums: Vec<String>,
    pub recent_artists: Vec<String>,
    pub recent_playlists: Vec<String>,
}
