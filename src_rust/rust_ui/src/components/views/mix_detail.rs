//! Mix detail page. Same shape as PlaylistDetail — header (cover + title
//! + sub-title) + a vertical track list — but the underlying endpoints
//! are the Mix flavor (`Session::fetch_mix`, `mix_items_list`). Mixed-
//! content videos drop out at the service layer.

use relm4::gtk::{
    self, prelude::*, Box as GtkBox, Image, Label, ListBox, Orientation, ScrolledWindow,
};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use rust_tidal_core::api::{Mix, Track};

use crate::components::views::common::{
    build_error_widget, build_loading_widget, build_track_row, LibraryViewOutput,
    ViewLoadState,
};
use crate::services::tidal_session::{spawn_blocking, TidalSessionService};

pub struct MixDetailInit {
    pub session: TidalSessionService,
    pub mix_id: String,
    pub initial_title: String,
}

pub struct MixDetailViewModel {
    session: TidalSessionService,
    mix_id: String,
    initial_title: String,
    mix: Option<Mix>,
    tracks: Vec<Track>,
    state: ViewLoadState,
    fetch_token: u64,
    header_done: bool,
    tracks_done: bool,
}

#[derive(Debug, Clone)]
pub enum MixDetailInput {
    Refresh,
    HeaderResult { token: u64, mix: Mix },
    HeaderFailed { token: u64, error: String },
    TracksResult { token: u64, items: Vec<Track> },
    TracksFailed { token: u64, error: String },
    /// Index into `tracks` — handler emits PlayContext with the mix
    /// as the queue source.
    Play(usize),
}

pub struct MixDetailWidgets {
    root: ScrolledWindow,
    body: GtkBox,
}

impl SimpleComponent for MixDetailViewModel {
    type Init = MixDetailInit;
    type Input = MixDetailInput;
    type Output = LibraryViewOutput;
    type Root = ScrolledWindow;
    type Widgets = MixDetailWidgets;

    fn init_root() -> Self::Root {
        ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .hexpand(true)
            .build()
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let body = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(16)
            .margin_top(16)
            .margin_bottom(16)
            .margin_start(16)
            .margin_end(16)
            .build();
        root.set_child(Some(&body));

        let model = Self {
            session: init.session,
            mix_id: init.mix_id,
            initial_title: init.initial_title,
            mix: None,
            tracks: Vec::new(),
            state: ViewLoadState::Idle,
            fetch_token: 0,
            header_done: false,
            tracks_done: false,
        };
        let widgets = MixDetailWidgets {
            root: root.clone(),
            body,
        };
        sender.input_sender().send(MixDetailInput::Refresh).ok();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            MixDetailInput::Refresh => {
                if matches!(self.state, ViewLoadState::Loading) {
                    return;
                }
                self.state = ViewLoadState::Loading;
                self.header_done = false;
                self.tracks_done = false;
                self.fetch_token = self.fetch_token.wrapping_add(1);
                let token = self.fetch_token;
                let svc1 = self.session.clone();
                let svc2 = self.session.clone();
                let id1 = self.mix_id.clone();
                let id2 = self.mix_id.clone();
                let s1 = sender.input_sender().clone();
                let s2 = sender.input_sender().clone();
                spawn_blocking(
                    move || svc1.fetch_mix_blocking(&id1),
                    move |result| {
                        let msg = match result {
                            Ok(mix) => MixDetailInput::HeaderResult { token, mix },
                            Err(e) => MixDetailInput::HeaderFailed {
                                token,
                                error: e.to_string(),
                            },
                        };
                        let _ = s1.send(msg);
                    },
                );
                spawn_blocking(
                    move || svc2.list_mix_tracks_blocking(&id2),
                    move |result| {
                        let msg = match result {
                            Ok(items) => MixDetailInput::TracksResult { token, items },
                            Err(e) => MixDetailInput::TracksFailed {
                                token,
                                error: e.to_string(),
                            },
                        };
                        let _ = s2.send(msg);
                    },
                );
            }
            MixDetailInput::HeaderResult { token, mix } => {
                if token != self.fetch_token {
                    return;
                }
                self.mix = Some(mix);
                self.header_done = true;
                self.maybe_finish();
            }
            MixDetailInput::HeaderFailed { token, error } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::warn!(mix = %self.mix_id, error = %error, "mix header fetch failed");
                self.header_done = true;
                self.maybe_finish();
            }
            MixDetailInput::TracksResult { token, items } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::info!(mix = %self.mix_id, count = items.len(), "mix tracks loaded");
                self.tracks = items;
                self.tracks_done = true;
                self.maybe_finish();
            }
            MixDetailInput::TracksFailed { token, error } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::warn!(mix = %self.mix_id, error = %error, "mix tracks fetch failed");
                self.state = ViewLoadState::Failed(error);
                self.tracks_done = true;
            }
            MixDetailInput::Play(idx) => {
                if idx < self.tracks.len() {
                    let title = self
                        .mix
                        .as_ref()
                        .map(|m| m.title.clone())
                        .unwrap_or_else(|| self.initial_title.clone());
                    let _ = sender.output(LibraryViewOutput::PlayContext {
                        tracks: self.tracks.clone(),
                        start_index: idx,
                        source: crate::state::playback::PlaybackSource::Mix {
                            id: self.mix_id.clone(),
                            title,
                        },
                    });
                }
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, sender: ComponentSender<Self>) {
        clear_box(&widgets.body);

        match &self.state {
            ViewLoadState::Idle | ViewLoadState::Loading => {
                widgets.root.set_child(Some(&build_loading_widget()));
                return;
            }
            ViewLoadState::Failed(err) => {
                widgets.root.set_child(Some(&build_error_widget(err)));
                return;
            }
            ViewLoadState::Loaded => {
                widgets.root.set_child(Some(&widgets.body));
            }
        }

        widgets
            .body
            .append(&build_header(self.mix.as_ref(), &self.initial_title));

        let list = ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["track-list"])
            .build();
        for (idx, track) in self.tracks.iter().enumerate() {
            let s = sender.clone();
            let row = build_track_row(idx + 1, track, move || {
                let _ = s.input_sender().send(MixDetailInput::Play(idx));
            });
            list.append(&row);
        }
        widgets.body.append(&list);
    }
}

impl MixDetailViewModel {
    fn maybe_finish(&mut self) {
        if self.header_done && self.tracks_done && !matches!(self.state, ViewLoadState::Failed(_))
        {
            self.state = ViewLoadState::Loaded;
        }
    }
}

fn clear_box(b: &GtkBox) {
    while let Some(child) = b.first_child() {
        b.remove(&child);
    }
}

fn build_header(mix: Option<&Mix>, initial_title: &str) -> GtkBox {
    let header = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(20)
        .margin_bottom(8)
        .build();

    let cover = Image::builder()
        .icon_name("media-playback-start-symbolic")
        .pixel_size(180)
        .css_classes(["album-cover-img"])
        .build();
    header.append(&cover);

    let info = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .vexpand(true)
        .valign(gtk::Align::Center)
        .build();

    let title = mix
        .map(|m| {
            if m.title.is_empty() {
                initial_title.to_string()
            } else {
                m.title.clone()
            }
        })
        .unwrap_or_else(|| initial_title.to_string());
    let title_label = Label::builder()
        .label(&title)
        .xalign(0.0)
        .css_classes(["title-1"])
        .wrap(true)
        .build();
    info.append(&title_label);

    if let Some(m) = mix {
        if let Some(sub) = m.sub_title.as_deref().filter(|s| !s.is_empty()) {
            let sub_label = Label::builder()
                .label(sub)
                .xalign(0.0)
                .css_classes(["title-3", "dim-label"])
                .build();
            info.append(&sub_label);
        }
        if let Some(kind) = m.mix_type.as_deref().filter(|s| !s.is_empty()) {
            let kind_label = Label::builder()
                .label(kind)
                .xalign(0.0)
                .css_classes(["dim-label", "caption"])
                .build();
            info.append(&kind_label);
        }
    }

    header.append(&info);
    header
}

