//! Playlist detail page. Header (cover + title + creator + track count)
//! plus a vertical track list. Mirrors `album_detail` but pulls from
//! `Session::fetch_playlist` + paginated `playlist_tracks`. Mixed
//! track/video items get filtered to tracks only — Phase 8 will add
//! a video tile when the playlist actually contains them.

use relm4::gtk::{
    self, prelude::*, Box as GtkBox, Image, Label, ListBox, Orientation, ScrolledWindow,
};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use rust_tidal_core::api::{Playlist, Track};

use crate::components::views::common::{
    build_error_widget, build_loading_widget, build_track_row, LibraryViewOutput,
    ViewLoadState,
};
use crate::services::tidal_session::{spawn_blocking, TidalSessionService};

pub struct PlaylistDetailInit {
    pub session: TidalSessionService,
    pub playlist_id: String,
    pub initial_title: String,
}

pub struct PlaylistDetailViewModel {
    session: TidalSessionService,
    playlist_id: String,
    initial_title: String,
    playlist: Option<Playlist>,
    tracks: Vec<Track>,
    state: ViewLoadState,
    fetch_token: u64,
    header_done: bool,
    tracks_done: bool,
}

#[derive(Debug, Clone)]
pub enum PlaylistDetailInput {
    Refresh,
    HeaderResult { token: u64, playlist: Playlist },
    HeaderFailed { token: u64, error: String },
    TracksResult { token: u64, items: Vec<Track> },
    TracksFailed { token: u64, error: String },
    Play(i64),
}

pub struct PlaylistDetailWidgets {
    root: ScrolledWindow,
    body: GtkBox,
}

impl SimpleComponent for PlaylistDetailViewModel {
    type Init = PlaylistDetailInit;
    type Input = PlaylistDetailInput;
    type Output = LibraryViewOutput;
    type Root = ScrolledWindow;
    type Widgets = PlaylistDetailWidgets;

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
            playlist_id: init.playlist_id,
            initial_title: init.initial_title,
            playlist: None,
            tracks: Vec::new(),
            state: ViewLoadState::Idle,
            fetch_token: 0,
            header_done: false,
            tracks_done: false,
        };
        let widgets = PlaylistDetailWidgets {
            root: root.clone(),
            body,
        };
        sender
            .input_sender()
            .send(PlaylistDetailInput::Refresh)
            .ok();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            PlaylistDetailInput::Refresh => {
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
                let id1 = self.playlist_id.clone();
                let id2 = self.playlist_id.clone();
                let s1 = sender.input_sender().clone();
                let s2 = sender.input_sender().clone();
                spawn_blocking(
                    move || svc1.fetch_playlist_blocking(&id1),
                    move |result| {
                        let msg = match result {
                            Ok(playlist) => PlaylistDetailInput::HeaderResult { token, playlist },
                            Err(e) => PlaylistDetailInput::HeaderFailed {
                                token,
                                error: e.to_string(),
                            },
                        };
                        let _ = s1.send(msg);
                    },
                );
                spawn_blocking(
                    move || svc2.list_playlist_tracks_blocking(&id2),
                    move |result| {
                        let msg = match result {
                            Ok(items) => PlaylistDetailInput::TracksResult { token, items },
                            Err(e) => PlaylistDetailInput::TracksFailed {
                                token,
                                error: e.to_string(),
                            },
                        };
                        let _ = s2.send(msg);
                    },
                );
            }
            PlaylistDetailInput::HeaderResult { token, playlist } => {
                if token != self.fetch_token {
                    return;
                }
                self.playlist = Some(playlist);
                self.header_done = true;
                self.maybe_finish();
            }
            PlaylistDetailInput::HeaderFailed { token, error } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::warn!(playlist = %self.playlist_id, error = %error, "playlist header fetch failed");
                self.header_done = true;
                self.maybe_finish();
            }
            PlaylistDetailInput::TracksResult { token, items } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::info!(playlist = %self.playlist_id, count = items.len(), "playlist tracks loaded");
                self.tracks = items;
                self.tracks_done = true;
                self.maybe_finish();
            }
            PlaylistDetailInput::TracksFailed { token, error } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::warn!(playlist = %self.playlist_id, error = %error, "playlist tracks fetch failed");
                self.state = ViewLoadState::Failed(error);
                self.tracks_done = true;
            }
            PlaylistDetailInput::Play(track_id) => {
                let _ = sender.output(LibraryViewOutput::PlayTrack { track_id });
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

        widgets.body.append(&build_header(
            self.playlist.as_ref(),
            &self.initial_title,
        ));

        let list = ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["track-list"])
            .build();
        for (idx, track) in self.tracks.iter().enumerate() {
            let s = sender.clone();
            let row = build_track_row(idx + 1, track, move |id| {
                let _ = s.input_sender().send(PlaylistDetailInput::Play(id));
            });
            list.append(&row);
        }
        widgets.body.append(&list);
    }
}

impl PlaylistDetailViewModel {
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

fn build_header(playlist: Option<&Playlist>, initial_title: &str) -> GtkBox {
    let header = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(20)
        .margin_bottom(8)
        .build();

    let cover = Image::builder()
        .icon_name("view-list-symbolic")
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

    let title = playlist
        .map(|p| {
            if p.name.is_empty() {
                initial_title.to_string()
            } else {
                p.name.clone()
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

    if let Some(p) = playlist {
        if let Some(creator) = p.creator_name.as_deref().filter(|s| !s.is_empty()) {
            let creator_label = Label::builder()
                .label(format!("by {creator}"))
                .xalign(0.0)
                .css_classes(["title-3", "dim-label"])
                .build();
            info.append(&creator_label);
        }
        if let Some(desc) = p.description.as_deref().filter(|s| !s.is_empty()) {
            let desc_label = Label::builder()
                .label(desc)
                .xalign(0.0)
                .wrap(true)
                .max_width_chars(60)
                .css_classes(["dim-label"])
                .build();
            info.append(&desc_label);
        }
        let mut bits: Vec<String> = Vec::new();
        if let Some(n) = p.num_tracks.filter(|n| *n > 0) {
            bits.push(format!("{n} tracks"));
        }
        if let Some(d) = p.duration.filter(|d| *d > 0) {
            let mins = d / 60;
            bits.push(format!("{mins} min"));
        }
        if !bits.is_empty() {
            let meta_label = Label::builder()
                .label(bits.join(" • "))
                .xalign(0.0)
                .css_classes(["dim-label", "caption"])
                .build();
            info.append(&meta_label);
        }
    }

    header.append(&info);
    header
}

