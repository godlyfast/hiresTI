//! Compact LUFS / dynamic-range readout for the mini-player area.
//! Polled on the same VizTick the bars use — AppController feeds the
//! latest `EngineLufsValues` snapshot in via `DrMeterInput::Set`.
//!
//! Layout is a single horizontal row:
//!   M  -23.4   S  -22.1   I  -22.0   LRA  6.4   DR  9.2
//! …with `--.-` shown for any field the engine reports as
//! NEG_INFINITY / 0 (no data yet).

use relm4::gtk::prelude::*;
use relm4::gtk::{Box as GtkBox, Label, Orientation};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use rust_audio_core::EngineLufsValues;

#[derive(Debug, Clone)]
pub enum DrMeterInput {
    /// New LUFS snapshot from the engine.
    Set(EngineLufsValues),
    /// Drop all readouts back to "--.-". Fires on stop / track change
    /// so stale numbers don't linger between tracks.
    Reset,
}

pub struct DrMeterModel {
    momentary: Label,
    short_term: Label,
    integrated: Label,
    lra: Label,
    dr: Label,
}

impl SimpleComponent for DrMeterModel {
    type Init = ();
    type Input = DrMeterInput;
    type Output = ();
    type Root = GtkBox;
    type Widgets = ();

    fn init_root() -> Self::Root {
        GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .css_classes(["dr-meter"])
            .margin_start(8)
            .margin_end(8)
            .margin_top(2)
            .margin_bottom(2)
            .build()
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let make_pair = |label: &str| -> (GtkBox, Label) {
            let row = GtkBox::builder()
                .orientation(Orientation::Horizontal)
                .spacing(4)
                .build();
            let key = Label::builder()
                .label(label)
                .css_classes(["dim-label", "caption"])
                .build();
            let val = Label::builder()
                .label("--.-")
                .css_classes(["heading", "monospace"])
                .build();
            row.append(&key);
            row.append(&val);
            (row, val)
        };

        let (m_row, momentary) = make_pair("M");
        let (s_row, short_term) = make_pair("S");
        let (i_row, integrated) = make_pair("I");
        let (lra_row, lra) = make_pair("LRA");
        let (dr_row, dr) = make_pair("DR");

        root.append(&m_row);
        root.append(&s_row);
        root.append(&i_row);
        root.append(&lra_row);
        root.append(&dr_row);

        let model = Self {
            momentary,
            short_term,
            integrated,
            lra,
            dr,
        };
        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            DrMeterInput::Set(v) => {
                self.momentary.set_label(&fmt_lufs(v.momentary));
                self.short_term.set_label(&fmt_lufs(v.short_term));
                self.integrated.set_label(&fmt_lufs(v.integrated));
                self.lra.set_label(&fmt_db(v.lra));
                self.dr.set_label(&fmt_db(v.dr));
            }
            DrMeterInput::Reset => {
                self.momentary.set_label("--.-");
                self.short_term.set_label("--.-");
                self.integrated.set_label("--.-");
                self.lra.set_label("--.-");
                self.dr.set_label("--.-");
            }
        }
    }
}

/// LUFS field formatter. NEG_INFINITY / unset → "--.-"; otherwise one
/// decimal with a sign, e.g. "-23.4".
fn fmt_lufs(v: f32) -> String {
    if !v.is_finite() || v <= -70.0 {
        "--.-".into()
    } else {
        format!("{v:.1}")
    }
}

/// dB-LU field formatter for LRA / DR. 0.0 means unavailable in the
/// engine contract; show a placeholder until the meter has enough
/// history.
fn fmt_db(v: f32) -> String {
    if !v.is_finite() || v <= 0.0 {
        "--.-".into()
    } else {
        format!("{v:.1}")
    }
}
