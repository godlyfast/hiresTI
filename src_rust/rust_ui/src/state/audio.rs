//! Audio output configuration. Maps the cluster of `settings.json`
//! fields the audio engine reads (driver / device / latency / DSP chain
//! / bit-perfect / exclusive) into a single typed struct.
//!
//! Phase 4 will derive this from `Settings` on app start and apply it to
//! the `rust_audio_core` engine. For now the type just exists so other
//! state code can refer to it.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AudioConfig {
    pub driver: String,
    pub device: String,
    pub bit_perfect: bool,
    pub exclusive_lock: bool,
    pub latency_profile: String,
    pub alsa_mmap_realtime_priority: String,
    pub output_bit_depth: String,
    /// 0..=100; the audio engine maps to its own internal range.
    pub volume: u8,
    /// DSP module ordering, e.g. ["peq", "convolver", "tape", "tube", "widener"].
    /// Empty means use defaults.
    pub dsp_order: Vec<String>,
}
