//! Bottom mini player. Phase 3 wires the layout (cover slot + title /
//! artist labels + transport controls) and surfaces button clicks as
//! `MiniPlayerOutput::*`. The actual playback driver lands in Phase 4
//! when we hook up `rust_audio_core` via the Rust API.

use relm4::gtk::{
    self, prelude::*, Box as GtkBox, Button, Image, Label, Orientation, Scale,
};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

pub struct MiniPlayerModel {
    title: String,
    artist: String,
    /// Position fraction 0.0..=1.0; the seek scale binds to this.
    progress: f64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // both senders wire in Phase 4 (audio engine)
pub enum MiniPlayerInput {
    SetNowPlaying { title: String, artist: String },
    SetProgress(f64),
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Pause is emitted by Phase 4 once transport state lands
pub enum MiniPlayerOutput {
    Play,
    Pause,
    Next,
    Previous,
    Seek(f64),
}

pub struct MiniPlayerWidgets {
    title_label: Label,
    artist_label: Label,
    seek: Scale,
}

impl SimpleComponent for MiniPlayerModel {
    type Init = ();
    type Input = MiniPlayerInput;
    type Output = MiniPlayerOutput;
    type Root = GtkBox;
    type Widgets = MiniPlayerWidgets;

    fn init_root() -> Self::Root {
        GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(12)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(12)
            .margin_end(12)
            .css_classes(["mini-player"])
            .build()
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // Cover art slot. Real artwork loading wires in Phase 7.
        let cover = Image::builder()
            .icon_name("audio-x-generic-symbolic")
            .pixel_size(48)
            .css_classes(["mini-player-cover"])
            .build();
        root.append(&cover);

        // Title + artist column.
        let info = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .hexpand(true)
            .build();
        let title_label = Label::builder()
            .label("Nothing playing")
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["heading"])
            .build();
        let artist_label = Label::builder()
            .label("")
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["dim-label", "caption"])
            .build();
        info.append(&title_label);
        info.append(&artist_label);

        // Seek scale below the labels.
        let seek = Scale::builder()
            .orientation(Orientation::Horizontal)
            .draw_value(false)
            .build();
        seek.set_range(0.0, 1.0);
        seek.set_value(0.0);
        let s = sender.clone();
        seek.connect_change_value(move |_scale, _scroll, value| {
            let _ = s.output(MiniPlayerOutput::Seek(value.clamp(0.0, 1.0)));
            glib::Propagation::Proceed
        });
        info.append(&seek);
        root.append(&info);

        // Transport buttons.
        let prev_btn = Button::builder()
            .icon_name("media-skip-backward-symbolic")
            .tooltip_text("Previous")
            .build();
        let s = sender.clone();
        prev_btn.connect_clicked(move |_| {
            let _ = s.output(MiniPlayerOutput::Previous);
        });
        root.append(&prev_btn);

        let play_btn = Button::builder()
            .icon_name("media-playback-start-symbolic")
            .tooltip_text("Play")
            .css_classes(["circular", "suggested-action"])
            .build();
        let s = sender.clone();
        play_btn.connect_clicked(move |_| {
            // Phase 3: only emit Play. Pause/play toggle wires in Phase 4
            // when transport state is real.
            let _ = s.output(MiniPlayerOutput::Play);
        });
        root.append(&play_btn);

        let next_btn = Button::builder()
            .icon_name("media-skip-forward-symbolic")
            .tooltip_text("Next")
            .build();
        let s = sender.clone();
        next_btn.connect_clicked(move |_| {
            let _ = s.output(MiniPlayerOutput::Next);
        });
        root.append(&next_btn);

        let model = Self {
            title: "Nothing playing".into(),
            artist: String::new(),
            progress: 0.0,
        };
        let widgets = MiniPlayerWidgets {
            title_label,
            artist_label,
            seek,
        };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            MiniPlayerInput::SetNowPlaying { title, artist } => {
                self.title = title;
                self.artist = artist;
            }
            MiniPlayerInput::SetProgress(p) => {
                self.progress = p.clamp(0.0, 1.0);
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        widgets.title_label.set_label(&self.title);
        widgets.artist_label.set_label(&self.artist);
        widgets.seek.set_value(self.progress);
    }
}

// glib re-export so the connect_change_value closure can return Propagation
// without an extra import path.
use relm4::gtk::glib;
