//! Visualizer config (independent of audio output config since the
//! visualizer can run without audio output, e.g. while paused).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VizConfig {
    /// Effect index 0..27 — same numbering the Python dispatcher uses
    /// (effect 4 = dot matrix, effect 14/15/16 = pro analyzer, etc.).
    pub effect: u8,
    /// Visualizer profile preset (decay/attack curves).
    pub profile: u8,
    /// Spectrum theme (gradient palette).
    pub theme: u8,
    /// Bar count: typically 32 / 64 / 96 / 128 / 256.
    pub bar_count: u16,
    /// 0 = log, 1 = linear.
    pub frequency_scale: u8,
    /// Whether the viz is currently expanded into the main panel vs.
    /// hidden behind the cover-art view.
    pub expanded: bool,
    /// Per-device A/V sync offset (ms). Stored as a JSON map
    /// `{ "FIIO KA13": 12, ... }` in settings.json.
    pub sync_offset_ms: i32,
}
