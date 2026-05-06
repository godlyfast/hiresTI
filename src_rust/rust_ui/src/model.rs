//! Root application state. Composes the per-domain sub-states from
//! `state/` into a single `AppModel` that the root Relm4 component owns.
//!
//! Sub-models are kept separate from widget code: the audio engine
//! produces `PlaybackState` updates without ever touching a widget,
//! and view components read whichever slice they care about. This is
//! the explicit corrective for the Python god-class pattern, where
//! every action handler had implicit access to every widget plus
//! every state field.

use crate::messages::NavTarget;
use crate::settings::Settings;
use crate::state::audio::AudioConfig;
use crate::state::auth::AuthState;
use crate::state::history::LocalHistoryState;
use crate::state::library::LibraryState;
use crate::state::nav::NavigationState;
use crate::state::playback::{PlaybackState, QueueState};
use crate::state::search::SearchState;
use crate::state::viz::VizConfig;

#[derive(Debug, Clone, Default)]
#[allow(dead_code)] // sub-fields wired in Phase 3+; placeholder slots ok now
pub struct AppModel {
    pub settings: Settings,
    pub current_nav: NavTarget,

    pub auth: AuthState,
    pub library: LibraryState,
    pub playback: PlaybackState,
    pub queue: QueueState,
    pub navigation: NavigationState,
    pub search: SearchState,
    pub history: LocalHistoryState,
    pub audio: AudioConfig,
    pub viz: VizConfig,
}

impl AppModel {
    /// Construct the root model from a freshly loaded `Settings`. Sub-
    /// states are at their defaults; data fetching kicks off in Phase 4
    /// once the auth flow lands.
    pub fn from_settings(settings: Settings) -> Self {
        let current_nav =
            NavTarget::from_id(&settings.last_nav).unwrap_or(NavTarget::Home);

        // Lift the audio + viz fields out of `settings.extra` if present.
        // We don't fail loudly if a field is missing — defaults take over.
        let audio = derive_audio_config(&settings);
        let viz = derive_viz_config(&settings);

        Self {
            settings,
            current_nav,
            audio,
            viz,
            ..Self::default()
        }
    }
}

fn derive_audio_config(s: &Settings) -> AudioConfig {
    AudioConfig {
        driver: extra_str(s, "driver").unwrap_or_else(|| "Auto (Default)".into()),
        device: extra_str(s, "device").unwrap_or_else(|| "Default Output".into()),
        bit_perfect: extra_bool(s, "bit_perfect").unwrap_or(false),
        exclusive_lock: extra_bool(s, "exclusive_lock").unwrap_or(false),
        latency_profile: extra_str(s, "latency_profile")
            .unwrap_or_else(|| "Standard (100ms)".into()),
        alsa_mmap_realtime_priority: extra_str(s, "alsa_mmap_realtime_priority")
            .unwrap_or_else(|| "High (60)".into()),
        output_bit_depth: extra_str(s, "output_bit_depth").unwrap_or_else(|| "Auto".into()),
        volume: extra_i64(s, "volume").map(|v| v.clamp(0, 100) as u8).unwrap_or(80),
        dsp_order: extra_str_array(s, "dsp_order").unwrap_or_default(),
    }
}

fn derive_viz_config(s: &Settings) -> VizConfig {
    VizConfig {
        effect: extra_i64(s, "viz_effect").map(|v| v.max(0) as u8).unwrap_or(3),
        profile: extra_i64(s, "viz_profile").map(|v| v.max(0) as u8).unwrap_or(2),
        theme: extra_i64(s, "spectrum_theme").map(|v| v.max(0) as u8).unwrap_or(0),
        bar_count: extra_i64(s, "viz_bar_count").map(|v| v.clamp(8, 1024) as u16).unwrap_or(32),
        frequency_scale: extra_i64(s, "viz_frequency_scale").map(|v| v.max(0) as u8).unwrap_or(0),
        expanded: extra_bool(s, "viz_expanded").unwrap_or(false),
        sync_offset_ms: extra_i64(s, "viz_sync_offset_ms").map(|v| v as i32).unwrap_or(0),
    }
}

fn extra_str(s: &Settings, key: &str) -> Option<String> {
    s.extra.get(key)?.as_str().map(|v| v.to_owned())
}
fn extra_bool(s: &Settings, key: &str) -> Option<bool> {
    s.extra.get(key)?.as_bool()
}
fn extra_i64(s: &Settings, key: &str) -> Option<i64> {
    s.extra.get(key)?.as_i64()
}
fn extra_str_array(s: &Settings, key: &str) -> Option<Vec<String>> {
    let arr = s.extra.get(key)?.as_array()?;
    Some(
        arr.iter()
            .filter_map(|v| v.as_str().map(|s| s.to_owned()))
            .collect(),
    )
}
