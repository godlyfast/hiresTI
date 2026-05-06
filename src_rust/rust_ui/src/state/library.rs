//! User library state: favorites + cached collections.
//!
//! The Python TidalBackend maintained four `fav_*_ids` sets plus separate
//! cached collections for the matching detail views. We model the same
//! shape but with explicit "is loaded?" tracking so the UI can show a
//! skeleton state vs. an "empty" state correctly.

use std::collections::HashSet;

use rust_tidal_core::api::{Album, Artist, Mix, Playlist, Track};

/// Set of TIDAL ids the user has favorited. Album / Track / Artist ids
/// are integers in TIDAL's API; Playlist + Mix use string uuids. We
/// store everything as `String` so a single set per kind is enough.
#[derive(Debug, Clone, Default)]
pub struct Favorites {
    pub album_ids: HashSet<String>,
    pub track_ids: HashSet<String>,
    pub artist_ids: HashSet<String>,
    pub playlist_ids: HashSet<String>,
    pub mix_ids: HashSet<String>,
}

impl Favorites {
    pub fn is_album(&self, id: &str) -> bool {
        self.album_ids.contains(id)
    }
    pub fn is_track(&self, id: &str) -> bool {
        self.track_ids.contains(id)
    }
    pub fn is_artist(&self, id: &str) -> bool {
        self.artist_ids.contains(id)
    }
    pub fn is_playlist(&self, id: &str) -> bool {
        self.playlist_ids.contains(id)
    }
    pub fn is_mix(&self, id: &str) -> bool {
        self.mix_ids.contains(id)
    }
}

/// Generic load-state for a cached collection. Lets the UI distinguish
/// "haven't asked yet" from "fetched and got nothing back".
#[derive(Debug, Clone, Default)]
pub enum LoadState<T> {
    #[default]
    Idle,
    Loading,
    Loaded(T),
    Failed(String),
}

impl<T> LoadState<T> {
    pub fn is_loaded(&self) -> bool {
        matches!(self, Self::Loaded(_))
    }
    pub fn data(&self) -> Option<&T> {
        match self {
            Self::Loaded(v) => Some(v),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct LibraryState {
    pub favorites: Favorites,

    // Cached collection lists. Each is updated when the user opens the
    // matching nav page. Detail-view caches (album→tracks, playlist→tracks)
    // live in their own per-view sub-models; these are just the index
    // collections.
    pub albums: LoadState<Vec<Album>>,
    pub tracks: LoadState<Vec<Track>>,
    pub artists: LoadState<Vec<Artist>>,
    pub playlists: LoadState<Vec<Playlist>>,
    pub mixes: LoadState<Vec<Mix>>,

    /// Dead-id cache: album ids TIDAL has returned 404 for. The Python
    /// version keeps these as a `set[int]` to short-circuit repeat
    /// fetches (see commit a0ef6b71). Same idea here.
    pub dead_album_ids: HashSet<String>,
    pub dead_track_ids: HashSet<String>,
}

impl LibraryState {
    pub fn mark_album_dead(&mut self, id: impl Into<String>) {
        self.dead_album_ids.insert(id.into());
    }
    pub fn mark_track_dead(&mut self, id: impl Into<String>) {
        self.dead_track_ids.insert(id.into());
    }
    pub fn is_album_dead(&self, id: &str) -> bool {
        self.dead_album_ids.contains(id)
    }
    pub fn is_track_dead(&self, id: &str) -> bool {
        self.dead_track_ids.contains(id)
    }
}
