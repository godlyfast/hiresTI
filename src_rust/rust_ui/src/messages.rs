//! Top-level Relm4 message enum. Every UI action that mutates the root
//! `AppModel` flows through one variant of `AppInput`. Sub-components
//! (sidebar, content stack, mini player, now playing) define their own
//! `Input`/`Output` enums and forward selected `Output` events here via
//! `forward()`.
//!
//! Phase 1 only carries variants the empty shell needs. Each subsequent
//! phase extends this list — no global registry, just `match` exhaustiveness
//! catches missing wirings at compile time.

// Phase 1 only constructs `ApplySettings` from app code; the other variants
// are wired in Phase 3 (sidebar) / Phase 4 (window-resize handler) so they
// would otherwise warn dead. Allow at the enum level — once each variant
// has a producer the lint clears naturally.
#![allow(dead_code)]

use crate::settings::Settings;

#[derive(Debug, Clone)]
pub enum AppInput {
    /// User clicked a top-level nav row (Home / Albums / Tracks / ...).
    /// The sidebar emits this; root persists it as `settings.last_nav`
    /// and switches the content stack.
    NavigateTo(NavTarget),

    /// The window was resized. Root persists width/height into settings
    /// when `remember_window_size` is on.
    WindowResized { width: i32, height: i32 },

    /// Settings mutated outside the UI flow (e.g. by an action handler).
    /// Root re-applies and persists.
    ApplySettings(Settings),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum NavTarget {
    #[default]
    Home,
    New,
    Top,
    HiRes,
    Genres,
    Decades,
    Moods,
    Albums,
    Tracks,
    Artists,
    Playlists,
    MixesAndRadio,
    History,
}

impl NavTarget {
    /// Stable string id used in `settings.last_nav` so existing user
    /// state survives the rewrite. Keep these strings in sync with
    /// the Python `NAV_KEYS` constants.
    pub fn as_id(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::New => "new",
            Self::Top => "top",
            Self::HiRes => "hires",
            Self::Genres => "genres",
            Self::Decades => "decades",
            Self::Moods => "moods",
            Self::Albums => "albums",
            Self::Tracks => "tracks",
            Self::Artists => "artists",
            Self::Playlists => "playlists",
            Self::MixesAndRadio => "mixes",
            Self::History => "history",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "home" => Self::Home,
            "new" => Self::New,
            "top" => Self::Top,
            "hires" => Self::HiRes,
            "genres" => Self::Genres,
            "decades" => Self::Decades,
            "moods" => Self::Moods,
            "albums" => Self::Albums,
            "tracks" => Self::Tracks,
            "artists" => Self::Artists,
            "playlists" => Self::Playlists,
            "mixes" => Self::MixesAndRadio,
            "history" => Self::History,
            _ => return None,
        })
    }

    /// Display label for sidebar rows. Will move to a localization
    /// table once we have one; raw English strings for Phase 1.
    pub fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::New => "New",
            Self::Top => "Top",
            Self::HiRes => "Hi-Res",
            Self::Genres => "Genres",
            Self::Decades => "Decades",
            Self::Moods => "Moods",
            Self::Albums => "Albums",
            Self::Tracks => "Tracks",
            Self::Artists => "Artists",
            Self::Playlists => "Playlists",
            Self::MixesAndRadio => "Mixes & Radio",
            Self::History => "History",
        }
    }
}

/// Visual grouping for the sidebar. Maps to the Python sidebar layout
/// (Discover / Your Library / Recent sections).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavSection {
    Discover,
    YourLibrary,
    Recent,
}

impl NavSection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Discover => "Discover",
            Self::YourLibrary => "Your Library",
            Self::Recent => "Recent",
        }
    }

    pub fn targets(self) -> &'static [NavTarget] {
        match self {
            Self::Discover => &[
                NavTarget::Home,
                NavTarget::New,
                NavTarget::Top,
                NavTarget::HiRes,
                NavTarget::Genres,
                NavTarget::Decades,
                NavTarget::Moods,
            ],
            Self::YourLibrary => &[
                NavTarget::Albums,
                NavTarget::Tracks,
                NavTarget::Artists,
                NavTarget::Playlists,
                NavTarget::MixesAndRadio,
            ],
            Self::Recent => &[NavTarget::History],
        }
    }

    pub const ALL: [NavSection; 3] = [Self::Discover, Self::YourLibrary, Self::Recent];
}
