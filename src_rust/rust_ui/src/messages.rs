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

    // ---- Header outputs --------------------------------------------
    Search(String),
    RequestLogin,
    OpenSettings,
    OpenAbout,
    OpenDiagnostics,

    // ---- Mini player outputs ---------------------------------------
    TransportPlay,
    TransportPause,
    TransportNext,
    TransportPrev,
    TransportSeek(f64),

    // ---- Auth flow (Phase 4) ---------------------------------------
    /// Result of the cold-start token restore attempt. None = no token
    /// on disk, or token rejected by /v1/sessions; Some = logged in.
    AuthRestoreResult(Option<crate::state::auth::UserProfile>),

    /// User pressed Login while logged in — log out instead.
    LogoutRequested,

    /// Device-code start finished. Show the dialog with the
    /// verification URL + user code; start polling.
    AuthDeviceStarted(rust_tidal_core::api::DeviceLogin),

    /// Device-code start failed. Surface the error in the header
    /// (and Phase 8 surfaces it in a notification toast).
    AuthDeviceStartFailed(String),

    /// One poll tick completed; either still waiting or logged in.
    AuthDevicePollTick(AuthPollOutcome),

    /// User closed the login dialog before authorizing — abort the
    /// pending poll loop.
    AuthDeviceCancelled,

    /// Open `url` in the user's default browser (xdg-open).
    OpenBrowser(String),

    /// Copy `text` to the system clipboard via the default Gdk display.
    CopyToClipboard(String),

    // ---- Library view outputs (Phase 5) ----------------------------
    OpenAlbum { id: String, title: String },
    OpenArtist { id: String, name: String },
    OpenPlaylist { uuid: String, title: String },
    OpenMix { id: String, title: String },
    PlayTrack { track_id: i64 },
    /// Play `tracks[start_index]` and load the rest as the queue. The
    /// payload is owned (Vec<Track>) so the originating view's local
    /// state is decoupled from the playback queue.
    PlayContext {
        tracks: Vec<rust_tidal_core::api::Track>,
        start_index: usize,
        source: crate::state::playback::PlaybackSource,
    },

    // ---- Detail navigation (Phase 7-A) -----------------------------
    /// Pop the currently-open detail surface and return to the active
    /// nav target. Header back-button + sidebar clicks both emit this.
    CloseDetail,

    // ---- Playback (Phase 7-C) --------------------------------------
    /// Stream resolution finished for a Play request. Carries enough
    /// info to populate the mini-player and (Phase 7-D) hand off to
    /// rust_audio_core. `request_id` matches what PlayTrack handed to
    /// the worker — stale resolves (the user clicked another track
    /// before this one came back) are dropped.
    NowPlayingResolved {
        request_id: u64,
        resolved: crate::services::tidal_session::ResolvedPlayback,
    },
    NowPlayingFailed {
        request_id: u64,
        error: String,
    },
    /// Periodic timer tick — recomputes the seek-bar position from the
    /// engine's current play head. Keeps the mini-player progress
    /// scrubbing in real time without per-decoded-frame events.
    PlaybackTick,
    /// Now-playing track's album cover finished downloading. The
    /// request_id matches the play counter so out-of-order resolves
    /// don't push a stale cover into the mini-player.
    NowPlayingCoverReady {
        request_id: u64,
        path: std::path::PathBuf,
    },
}

#[derive(Debug, Clone)]
pub enum AuthPollOutcome {
    StillPending,
    LoggedIn(crate::state::auth::UserProfile),
    Failed(String),
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
