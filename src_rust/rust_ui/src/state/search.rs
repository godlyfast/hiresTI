//! Search state: current query + multi-section results + recent queries.

use rust_tidal_core::api::{Album, Artist, Mix, Playlist, Track};

use crate::state::library::LoadState;

const MAX_HISTORY: usize = 25;

#[derive(Debug, Clone, Default)]
pub struct SearchResults {
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
    pub mixes: Vec<Mix>,
}

impl SearchResults {
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
            && self.albums.is_empty()
            && self.artists.is_empty()
            && self.playlists.is_empty()
            && self.mixes.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct SearchState {
    pub query: String,
    pub results: LoadState<SearchResults>,
    /// Persisted across runs via `settings.search_history`. Most recent
    /// first; capped at `MAX_HISTORY`.
    pub history: Vec<String>,
}

impl SearchState {
    /// Push `q` to the front of history; dedupe so re-searching an old
    /// term moves it to front rather than duplicating. Cap at MAX_HISTORY.
    pub fn record_query(&mut self, q: impl Into<String>) {
        let q = q.into();
        let trimmed = q.trim();
        if trimmed.is_empty() {
            return;
        }
        self.history.retain(|h| h != trimmed);
        self.history.insert(0, trimmed.into());
        if self.history.len() > MAX_HISTORY {
            self.history.truncate(MAX_HISTORY);
        }
    }
}
