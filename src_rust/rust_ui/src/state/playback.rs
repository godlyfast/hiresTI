//! Playback / queue state.
//!
//! The Python `TidalApp` carried these as flat attributes mixed with
//! widget refs; the rewrite separates them so the audio engine can drive
//! `PlaybackState` without ever touching widgets.

use std::time::Duration;

use rust_tidal_core::api::Track;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayMode {
    #[default]
    Normal,
    Loop,
    One,
    Shuffle,
    /// "Smart" mode in the Python version — adaptive shuffle that
    /// avoids recently-played tracks. Implementation lives in the
    /// queue manager (Phase 7).
    Smart,
}

impl PlayMode {
    pub const ALL: [PlayMode; 5] = [
        Self::Normal,
        Self::Loop,
        Self::One,
        Self::Shuffle,
        Self::Smart,
    ];

    /// Stable id used in `settings.json:play_mode` (the Python schema
    /// stores it as int 0..4 matching the order above).
    pub fn from_int(value: i32) -> Self {
        match value {
            1 => Self::Loop,
            2 => Self::One,
            3 => Self::Shuffle,
            4 => Self::Smart,
            _ => Self::Normal,
        }
    }
    pub fn to_int(self) -> i32 {
        match self {
            Self::Normal => 0,
            Self::Loop => 1,
            Self::One => 2,
            Self::Shuffle => 3,
            Self::Smart => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TransportState {
    #[default]
    Stopped,
    Playing,
    Paused,
    /// Mid stream-url resolution / format negotiation. UI typically shows
    /// a spinner.
    Buffering,
}

#[derive(Debug, Clone, Default)]
pub struct PlaybackState {
    pub transport: TransportState,
    pub current_track: Option<Track>,
    pub position: Duration,
    pub duration: Duration,
    /// Source of the current play queue: an album, playlist, mix, search
    /// result, etc. Used by Now Playing to render a "Playing from X"
    /// breadcrumb and by PlayMode::Smart to scope its history filter.
    pub source: PlaybackSource,
    pub volume: u8, // 0..=100
    pub muted: bool,
    pub mode: PlayMode,
}

#[derive(Debug, Clone, Default)]
pub enum PlaybackSource {
    #[default]
    None,
    Album { id: String, title: String },
    Playlist { uuid: String, title: String },
    Mix { id: String, title: String },
    Artist { id: String, name: String },
    LikedTracks,
    History,
    Search { query: String },
    /// Single-track play (e.g. clicked a track in a search row).
    SingleTrack,
}

impl PlaybackState {
    pub fn is_playing(&self) -> bool {
        matches!(self.transport, TransportState::Playing)
    }

    pub fn current_track_id(&self) -> Option<i64> {
        self.current_track.as_ref().map(|t| t.id)
    }

    pub fn position_fraction(&self) -> f32 {
        let dur = self.duration.as_secs_f32();
        if dur < 0.001 {
            return 0.0;
        }
        (self.position.as_secs_f32() / dur).clamp(0.0, 1.0)
    }
}

/// The play queue, ordered. `current_index` points into `tracks`. When
/// `PlayMode::Shuffle` is active, `shuffle_order` holds the permutation
/// the user is actually walking through; `tracks` stays in source order.
#[derive(Debug, Clone, Default)]
pub struct QueueState {
    pub tracks: Vec<Track>,
    pub current_index: usize,
    pub shuffle_order: Option<Vec<usize>>,
}

impl QueueState {
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn current(&self) -> Option<&Track> {
        let idx = self.physical_index(self.current_index)?;
        self.tracks.get(idx)
    }

    fn physical_index(&self, logical: usize) -> Option<usize> {
        if let Some(perm) = self.shuffle_order.as_ref() {
            perm.get(logical).copied()
        } else {
            Some(logical)
        }
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }
}
