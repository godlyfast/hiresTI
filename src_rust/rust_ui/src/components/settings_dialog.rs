//! Settings dialog. Adw.PreferencesWindow with one page covering the
//! audio-streaming + output controls Phase 8 needs to surface. The
//! dialog operates on a shared `Rc<RefCell<Settings>>` working clone so
//! several widgets can each mutate one field without losing earlier
//! edits — every change re-emits the full merged Settings as
//! `AppInput::ApplySettings(new)` for the root to persist + apply.
//!
//! Built imperatively rather than as a SimpleComponent because the
//! dialog is one-shot per click; the next open re-reads current
//! settings from scratch.

use std::cell::RefCell;
use std::rc::Rc;

use libadwaita::prelude::*;
use libadwaita::{
    ComboRow, EntryRow, PreferencesGroup, PreferencesPage, PreferencesWindow, SwitchRow,
};
use relm4::gtk::{StringList, Window};
use relm4::Sender;
use serde_json::Value;

use crate::messages::AppInput;
use crate::settings::Settings;

const QUALITY_OPTIONS: &[(&str, &str)] = &[
    ("HI_RES_LOSSLESS", "Hi-Res Lossless (FLAC up to 24/192)"),
    ("LOSSLESS", "Lossless (CD-quality FLAC)"),
    ("HIGH", "High (320kbps AAC)"),
    ("LOW", "Low (96kbps AAC)"),
];

const DRIVER_OPTIONS: &[&str] = &[
    "USB Rawlink v2",
    "ALSA",
    "ALSA mmap",
    "PulseAudio",
    "PipeWire",
];

const LATENCY_OPTIONS: &[&str] = &[
    "Aggressive (20ms)",
    "Balanced (60ms)",
    "Stable (100ms)",
];

const MMAP_PRIORITY_OPTIONS: &[&str] = &[
    "Off (0)",
    "Low (40)",
    "Medium (60)",
    "High (70)",
    "Very High (80)",
];

type Working = Rc<RefCell<Settings>>;

pub fn present(parent: &Window, current: &Settings, sender: Sender<AppInput>) {
    let working: Working = Rc::new(RefCell::new(current.clone()));

    let dialog = PreferencesWindow::builder()
        .modal(true)
        .transient_for(parent)
        .title("Settings")
        .default_width(640)
        .default_height(540)
        .build();

    let page = PreferencesPage::builder()
        .icon_name("audio-x-generic-symbolic")
        .title("Audio")
        .build();

    let streaming = PreferencesGroup::builder()
        .title("Streaming")
        .description("Quality TIDAL serves to the player")
        .build();
    streaming.add(&build_quality_row(&working, sender.clone()));
    page.add(&streaming);

    let output = PreferencesGroup::builder()
        .title("Audio Output")
        .description("Driver + device + bit-perfect playback")
        .build();
    output.add(&build_driver_row(&working, sender.clone()));
    output.add(&build_device_row(&working, sender.clone()));
    output.add(&build_switch_row(
        "Bit-perfect output",
        "Bypass software volume + format conversion",
        bool_from_settings(&working.borrow(), "bit_perfect", true),
        "bit_perfect",
        &working,
        sender.clone(),
    ));
    output.add(&build_switch_row(
        "Exclusive lock",
        "Hold the device exclusively while playing",
        bool_from_settings(&working.borrow(), "exclusive_lock", true),
        "exclusive_lock",
        &working,
        sender.clone(),
    ));
    output.add(&build_combo_row_static(
        "Latency profile",
        LATENCY_OPTIONS,
        str_from_settings(&working.borrow(), "latency_profile")
            .unwrap_or_else(|| LATENCY_OPTIONS[0].to_string()),
        "latency_profile",
        &working,
        sender.clone(),
    ));
    output.add(&build_combo_row_static(
        "ALSA mmap real-time priority",
        MMAP_PRIORITY_OPTIONS,
        str_from_settings(&working.borrow(), "alsa_mmap_realtime_priority")
            .unwrap_or_else(|| MMAP_PRIORITY_OPTIONS[3].to_string()),
        "alsa_mmap_realtime_priority",
        &working,
        sender,
    ));
    page.add(&output);

    dialog.add(&page);
    dialog.present();
}

fn str_from_settings(settings: &Settings, key: &str) -> Option<String> {
    settings
        .extra
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn bool_from_settings(settings: &Settings, key: &str, default: bool) -> bool {
    settings
        .extra
        .get(key)
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

fn dispatch_change(working: &Working, key: &str, value: Value, sender: &Sender<AppInput>) {
    let snapshot = {
        let mut w = working.borrow_mut();
        w.extra.insert(key.to_string(), value);
        w.clone()
    };
    let _ = sender.send(AppInput::ApplySettings(snapshot));
}

fn build_quality_row(working: &Working, sender: Sender<AppInput>) -> ComboRow {
    let labels: Vec<&str> = QUALITY_OPTIONS.iter().map(|(_, label)| *label).collect();
    let model = StringList::new(&labels);
    let active_id = str_from_settings(&working.borrow(), "audio_quality")
        .unwrap_or_else(|| QUALITY_OPTIONS[0].0.to_string());
    let active = QUALITY_OPTIONS
        .iter()
        .position(|(id, _)| *id == active_id)
        .unwrap_or(0) as u32;
    let row = ComboRow::builder()
        .title("Audio quality")
        .model(&model)
        .selected(active)
        .build();
    let working_c = Rc::clone(working);
    row.connect_selected_notify(move |r| {
        let idx = r.selected() as usize;
        let id = QUALITY_OPTIONS
            .get(idx)
            .map(|(id, _)| *id)
            .unwrap_or(QUALITY_OPTIONS[0].0);
        dispatch_change(&working_c, "audio_quality", Value::String(id.into()), &sender);
    });
    row
}

fn build_driver_row(working: &Working, sender: Sender<AppInput>) -> ComboRow {
    let model = StringList::new(DRIVER_OPTIONS);
    let active_label = str_from_settings(&working.borrow(), "driver")
        .unwrap_or_else(|| DRIVER_OPTIONS[0].to_string());
    let active = DRIVER_OPTIONS
        .iter()
        .position(|d| *d == active_label.as_str())
        .unwrap_or(0) as u32;
    let row = ComboRow::builder()
        .title("Output driver")
        .model(&model)
        .selected(active)
        .build();
    let working_c = Rc::clone(working);
    row.connect_selected_notify(move |r| {
        let idx = r.selected() as usize;
        let label = DRIVER_OPTIONS.get(idx).copied().unwrap_or(DRIVER_OPTIONS[0]);
        dispatch_change(&working_c, "driver", Value::String(label.into()), &sender);
    });
    row
}

fn build_device_row(working: &Working, sender: Sender<AppInput>) -> EntryRow {
    let row = EntryRow::builder()
        .title("Output device")
        .text(str_from_settings(&working.borrow(), "device").unwrap_or_default())
        .build();
    let working_c = Rc::clone(working);
    row.connect_apply(move |r| {
        let text = r.text().to_string();
        dispatch_change(&working_c, "device", Value::String(text), &sender);
    });
    row
}

fn build_switch_row(
    title: &str,
    subtitle: &str,
    initial: bool,
    key: &'static str,
    working: &Working,
    sender: Sender<AppInput>,
) -> SwitchRow {
    let row = SwitchRow::builder()
        .title(title)
        .subtitle(subtitle)
        .active(initial)
        .build();
    let working_c = Rc::clone(working);
    row.connect_active_notify(move |r| {
        let v = r.is_active();
        dispatch_change(&working_c, key, Value::Bool(v), &sender);
    });
    row
}

fn build_combo_row_static(
    title: &str,
    options: &'static [&'static str],
    initial: String,
    key: &'static str,
    working: &Working,
    sender: Sender<AppInput>,
) -> ComboRow {
    let model = StringList::new(options);
    let active = options
        .iter()
        .position(|s| *s == initial.as_str())
        .unwrap_or(0) as u32;
    let row = ComboRow::builder()
        .title(title)
        .model(&model)
        .selected(active)
        .build();
    let working_c = Rc::clone(working);
    row.connect_selected_notify(move |r| {
        let idx = r.selected() as usize;
        let label = options.get(idx).copied().unwrap_or(options[0]);
        dispatch_change(&working_c, key, Value::String(label.into()), &sender);
    });
    row
}
