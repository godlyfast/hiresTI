//! Signal path window. Horizontal flow of stage cards mirroring the
//! DSP graph rust_audio_core builds when a track plays:
//!
//!   Source → [Resampler] → [PEQ] → [Convolver] → [Tape] → [Tube]
//!          → [Widener]   → [Limiter] → Output
//!
//! Each card reads the matching `dsp_*_enabled` / `dsp_*_<param>` field
//! from settings and renders an enabled/disabled indicator + the key
//! parameter values. Live signal levels (peak meter, post-stage
//! sample rate, etc.) are Phase 9-A's job — this surface is the static
//! topology + config snapshot.

use libadwaita::prelude::*;
use libadwaita::Window;
use relm4::gtk::{
    self, Box as GtkBox, HeaderBar, Image, Label, Orientation, ScrolledWindow,
};

use crate::model::AppModel;

pub fn present(parent: &gtk::Window, model: &AppModel) {
    let dialog = Window::builder()
        .modal(false)
        .transient_for(parent)
        .title("Signal Path")
        .default_width(960)
        .default_height(360)
        .build();

    let bar = HeaderBar::new();
    let outer = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .build();
    outer.append(&bar);

    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .build();

    let chain = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .margin_top(20)
        .margin_bottom(20)
        .margin_start(20)
        .margin_end(20)
        .build();

    let stages = collect_stages(model);
    let n = stages.len();
    for (i, stage) in stages.into_iter().enumerate() {
        chain.append(&build_stage_card(&stage));
        if i + 1 < n {
            chain.append(&build_arrow());
        }
    }

    scroll.set_child(Some(&chain));
    outer.append(&scroll);
    dialog.set_content(Some(&outer));
    dialog.present();
}

struct Stage {
    title: String,
    icon: &'static str,
    enabled: Option<bool>,
    rows: Vec<(String, String)>,
}

fn collect_stages(model: &AppModel) -> Vec<Stage> {
    let mut out: Vec<Stage> = Vec::new();
    let s = &model.settings.extra;
    let bool_at = |k: &str| s.get(k).and_then(|v| v.as_bool());
    let i64_at = |k: &str| s.get(k).and_then(|v| v.as_i64());
    let str_at = |k: &str| {
        s.get(k)
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_default()
    };

    // Source — driven by current track when there's one. Always shown
    // as enabled because the chain has no upstream "off" state.
    let source_rows = match model.playback.current_track.as_ref() {
        Some(t) => vec![
            ("Track".into(), t.name.clone()),
            (
                "Quality".into(),
                t.audio_quality
                    .as_deref()
                    .unwrap_or("(unknown)")
                    .to_string(),
            ),
        ],
        None => vec![("Track".into(), "(nothing playing)".into())],
    };
    out.push(Stage {
        title: "Source".into(),
        icon: "music-note-symbolic",
        enabled: Some(true),
        rows: source_rows,
    });

    let dsp_master = bool_at("dsp_enabled").unwrap_or(false);

    // Resampler
    out.push(Stage {
        title: "Resampler".into(),
        icon: "preferences-system-symbolic",
        enabled: Some(dsp_master && bool_at("dsp_resampler_enabled").unwrap_or(false)),
        rows: vec![
            (
                "Target rate".into(),
                match i64_at("dsp_resampler_target_rate").unwrap_or(0) {
                    0 => "Source-rate".into(),
                    n => format!("{n} Hz"),
                },
            ),
            (
                "Quality".into(),
                i64_at("dsp_resampler_quality")
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "—".into()),
            ),
        ],
    });

    // PEQ
    let peq_band_count = s
        .get("dsp_peq_bands")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    out.push(Stage {
        title: "PEQ".into(),
        icon: "audio-volume-high-symbolic",
        enabled: Some(dsp_master && bool_at("dsp_peq_enabled").unwrap_or(false)),
        rows: vec![("Bands".into(), format!("{peq_band_count}"))],
    });

    // Convolver
    let ir_path = str_at("dsp_convolver_path");
    out.push(Stage {
        title: "Convolver".into(),
        icon: "view-mirror-symbolic",
        enabled: Some(dsp_master && bool_at("dsp_convolver_enabled").unwrap_or(false)),
        rows: vec![
            (
                "IR".into(),
                if ir_path.is_empty() {
                    "(none loaded)".into()
                } else {
                    file_basename(&ir_path)
                },
            ),
            (
                "Mix".into(),
                format!("{}%", i64_at("dsp_convolver_mix").unwrap_or(100)),
            ),
        ],
    });

    // Tape
    out.push(Stage {
        title: "Tape".into(),
        icon: "media-tape-symbolic",
        enabled: Some(dsp_master && bool_at("dsp_tape_enabled").unwrap_or(false)),
        rows: vec![
            ("Drive".into(), format!("{}", i64_at("dsp_tape_drive").unwrap_or(0))),
            ("Tone".into(), format!("{}", i64_at("dsp_tape_tone").unwrap_or(0))),
            (
                "Warmth".into(),
                format!("{}", i64_at("dsp_tape_warmth").unwrap_or(0)),
            ),
        ],
    });

    // Tube
    out.push(Stage {
        title: "Tube".into(),
        icon: "applications-utilities-symbolic",
        enabled: Some(dsp_master && bool_at("dsp_tube_enabled").unwrap_or(false)),
        rows: vec![
            ("Drive".into(), format!("{}", i64_at("dsp_tube_drive").unwrap_or(0))),
            ("Bias".into(), format!("{}", i64_at("dsp_tube_bias").unwrap_or(0))),
            ("Sag".into(), format!("{}", i64_at("dsp_tube_sag").unwrap_or(0))),
            ("Air".into(), format!("{}", i64_at("dsp_tube_air").unwrap_or(0))),
        ],
    });

    // Widener
    out.push(Stage {
        title: "Widener".into(),
        icon: "view-fullscreen-symbolic",
        enabled: Some(dsp_master && bool_at("dsp_widener_enabled").unwrap_or(false)),
        rows: vec![
            (
                "Width".into(),
                format!("{}", i64_at("dsp_widener_width").unwrap_or(100)),
            ),
            (
                "Bass mono".into(),
                format!("{} Hz", i64_at("dsp_widener_bass_mono_freq").unwrap_or(0)),
            ),
        ],
    });

    // Limiter
    out.push(Stage {
        title: "Limiter".into(),
        icon: "system-shutdown-symbolic",
        enabled: Some(dsp_master && bool_at("dsp_limiter_enabled").unwrap_or(false)),
        rows: vec![
            (
                "Threshold".into(),
                format!("{}", i64_at("dsp_limiter_threshold").unwrap_or(0)),
            ),
            (
                "Ratio".into(),
                format!("{}", i64_at("dsp_limiter_ratio").unwrap_or(0)),
            ),
        ],
    });

    // Output
    let driver = if model.audio.driver.is_empty() {
        str_at("driver")
    } else {
        model.audio.driver.clone()
    };
    let device = if model.audio.device.is_empty() {
        str_at("device")
    } else {
        model.audio.device.clone()
    };
    out.push(Stage {
        title: "Output".into(),
        icon: "audio-headphones-symbolic",
        enabled: Some(true),
        rows: vec![
            (
                "Driver".into(),
                if driver.is_empty() {
                    "(default)".into()
                } else {
                    driver
                },
            ),
            (
                "Device".into(),
                if device.is_empty() {
                    "(auto)".into()
                } else {
                    device
                },
            ),
            (
                "Bit-perfect".into(),
                if model.audio.bit_perfect { "yes" } else { "no" }.into(),
            ),
        ],
    });

    out
}

fn file_basename(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string())
}

fn build_stage_card(stage: &Stage) -> GtkBox {
    let card = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .width_request(180)
        .css_classes(["card", "signal-stage-card"])
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(4)
        .margin_end(4)
        .build();

    let head = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .margin_top(10)
        .margin_start(10)
        .margin_end(10)
        .build();
    let icon = Image::from_icon_name(stage.icon);
    icon.set_pixel_size(20);
    head.append(&icon);
    let title = Label::builder()
        .label(&stage.title)
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["heading"])
        .build();
    head.append(&title);
    let badge_text = match stage.enabled {
        Some(true) => "ON",
        Some(false) => "off",
        None => "—",
    };
    let badge_class = match stage.enabled {
        Some(true) => "success",
        _ => "dim-label",
    };
    let badge = Label::builder()
        .label(badge_text)
        .css_classes([badge_class, "caption"])
        .build();
    head.append(&badge);
    card.append(&head);

    for (key, val) in &stage.rows {
        let row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .margin_start(10)
            .margin_end(10)
            .build();
        let k = Label::builder()
            .label(key)
            .xalign(0.0)
            .css_classes(["caption", "dim-label"])
            .build();
        let v = Label::builder()
            .label(val)
            .xalign(1.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["caption"])
            .build();
        row.append(&k);
        row.append(&v);
        card.append(&row);
    }
    let pad = Label::builder()
        .label("")
        .margin_bottom(10)
        .build();
    card.append(&pad);
    card
}

fn build_arrow() -> Image {
    let arrow = Image::from_icon_name("go-next-symbolic");
    arrow.set_pixel_size(20);
    arrow.set_valign(gtk::Align::Center);
    arrow
}
