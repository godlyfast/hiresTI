//! Navigation state. The current top-level nav row + a back-stack of
//! visited views so the "back" button has somewhere to go.
//!
//! Replaces the ad-hoc `app.nav_history` list + `app.right_stack` page
//! name juggling that the Python version used.

use crate::messages::NavTarget;

/// A single entry on the back-stack — what the user is looking at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewLocation {
    /// Top-level nav grid / list (Albums grid, Playlists list, etc.)
    Nav(NavTarget),
    /// Album detail page.
    AlbumDetail { id: String },
    /// Playlist detail page.
    PlaylistDetail { uuid: String },
    /// Artist detail page.
    ArtistDetail { id: String },
    /// Mix detail page.
    MixDetail { id: String },
    /// Search results panel for a query.
    SearchResults { query: String },
    /// "Now Playing" full-screen takeover.
    NowPlaying,
}

#[derive(Debug, Clone, Default)]
pub struct NavigationState {
    pub current_nav: Option<NavTarget>,
    /// The currently-visible view (may be a detail view layered above
    /// the active nav target).
    pub current_view: Option<ViewLocation>,
    /// Back-stack — most recent at the end. Doesn't include
    /// `current_view`; pop a value off and make it current to go back.
    pub history: Vec<ViewLocation>,
}

impl NavigationState {
    pub fn navigate(&mut self, view: ViewLocation) {
        if let Some(prev) = self.current_view.take() {
            self.history.push(prev);
        }
        if let ViewLocation::Nav(target) = &view {
            self.current_nav = Some(*target);
        }
        self.current_view = Some(view);
    }

    pub fn pop(&mut self) -> Option<ViewLocation> {
        let prev = self.history.pop()?;
        if let ViewLocation::Nav(target) = &prev {
            self.current_nav = Some(*target);
        }
        let now = std::mem::replace(&mut self.current_view, Some(prev.clone()));
        // If we already had a current view, we discarded it (the back
        // button is destructive). The popped entry is now active.
        let _ = now;
        Some(prev)
    }

    pub fn can_go_back(&self) -> bool {
        !self.history.is_empty()
    }
}
