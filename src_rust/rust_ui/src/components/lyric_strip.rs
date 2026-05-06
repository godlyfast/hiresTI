//! Single-line synchronized-lyric overlay shown between the
//! visualizer/meter strip and the mini-player. Designed to be
//! cheap: AppController computes the active line on each
//! PlaybackTick and pushes the resolved text in via
//! `LyricStripInput::SetLine`. The widget itself does no parsing.

use relm4::gtk::prelude::*;
use relm4::gtk::{Box as GtkBox, Label, Orientation};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

#[derive(Debug, Clone)]
pub enum LyricStripInput {
    /// `None` clears the line (between songs, before the first
    /// timestamp, or when the track has no synced lyrics at all).
    SetLine(Option<String>),
}

pub struct LyricStripModel {
    label: Label,
    container: GtkBox,
}

impl SimpleComponent for LyricStripModel {
    type Init = ();
    type Input = LyricStripInput;
    type Output = ();
    type Root = GtkBox;
    type Widgets = ();

    fn init_root() -> Self::Root {
        GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .css_classes(["lyric-strip"])
            .margin_start(12)
            .margin_end(12)
            .margin_top(2)
            .margin_bottom(2)
            .visible(false)
            .build()
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let label = Label::builder()
            .label("")
            .css_classes(["heading"])
            .ellipsize(relm4::gtk::pango::EllipsizeMode::End)
            .single_line_mode(true)
            .xalign(0.0)
            .hexpand(true)
            .build();
        root.append(&label);
        let model = Self {
            label,
            container: root,
        };
        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            LyricStripInput::SetLine(line) => match line {
                Some(text) if !text.is_empty() => {
                    self.label.set_label(&text);
                    self.container.set_visible(true);
                }
                _ => {
                    self.label.set_label("");
                    self.container.set_visible(false);
                }
            },
        }
    }
}
