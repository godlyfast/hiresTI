//! Album detail page. Header (cover + title + artist + year + track count)
//! plus a vertical track list. The cover image is a placeholder until
//! Phase 7-D wires the resources.tidal.com fetcher.
//!
//! Two parallel fetches: `Session::fetch_album` for the header, and the
//! paginated `album_tracks` for the list. Each has its own fetch_token
//! so a Refresh while one is mid-flight invalidates both stale callbacks.

use relm4::gtk::{
    self, prelude::*, Box as GtkBox, Image, Label, ListBox, ListBoxRow, Orientation,
    ScrolledWindow,
};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use rust_tidal_core::api::{Album, Track};

use crate::components::views::common::{
    album_artist_name, build_error_widget, build_loading_widget, format_duration,
    track_artist_name, LibraryViewOutput, ViewLoadState,
};
use crate::services::tidal_session::{spawn_blocking, TidalSessionService};

pub struct AlbumDetailInit {
    pub session: TidalSessionService,
    pub album_id: i64,
    /// Title we already know from the caller (the album card the user
    /// clicked) — shown immediately while the header fetch resolves.
    pub initial_title: String,
}

pub struct AlbumDetailViewModel {
    session: TidalSessionService,
    album_id: i64,
    initial_title: String,
    album: Option<Album>,
    tracks: Vec<Track>,
    state: ViewLoadState,
    fetch_token: u64,
    /// Which fetches finished — both header + tracks must arrive (or fail)
    /// before we leave Loading. A header-only failure still surfaces the
    /// list, since the track list is the primary content.
    header_done: bool,
    tracks_done: bool,
}

#[derive(Debug, Clone)]
pub enum AlbumDetailInput {
    Refresh,
    HeaderResult { token: u64, album: Album },
    HeaderFailed { token: u64, error: String },
    TracksResult { token: u64, items: Vec<Track> },
    TracksFailed { token: u64, error: String },
    Play(i64),
    OpenArtist(i64, String),
}

pub struct AlbumDetailWidgets {
    root: ScrolledWindow,
    body: GtkBox,
}

impl SimpleComponent for AlbumDetailViewModel {
    type Init = AlbumDetailInit;
    type Input = AlbumDetailInput;
    type Output = LibraryViewOutput;
    type Root = ScrolledWindow;
    type Widgets = AlbumDetailWidgets;

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
            album_id: init.album_id,
            initial_title: init.initial_title,
            album: None,
            tracks: Vec::new(),
            state: ViewLoadState::Idle,
            fetch_token: 0,
            header_done: false,
            tracks_done: false,
        };
        let widgets = AlbumDetailWidgets {
            root: root.clone(),
            body,
        };
        // Auto-refresh on first launch — caller doesn't have to send Refresh.
        sender
            .input_sender()
            .send(AlbumDetailInput::Refresh)
            .ok();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            AlbumDetailInput::Refresh => {
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
                let id = self.album_id;
                let s1 = sender.input_sender().clone();
                let s2 = sender.input_sender().clone();
                spawn_blocking(
                    move || svc1.fetch_album_blocking(id),
                    move |result| {
                        let msg = match result {
                            Ok(album) => AlbumDetailInput::HeaderResult { token, album },
                            Err(e) => AlbumDetailInput::HeaderFailed {
                                token,
                                error: e.to_string(),
                            },
                        };
                        let _ = s1.send(msg);
                    },
                );
                spawn_blocking(
                    move || svc2.list_album_tracks_blocking(id),
                    move |result| {
                        let msg = match result {
                            Ok(items) => AlbumDetailInput::TracksResult { token, items },
                            Err(e) => AlbumDetailInput::TracksFailed {
                                token,
                                error: e.to_string(),
                            },
                        };
                        let _ = s2.send(msg);
                    },
                );
            }
            AlbumDetailInput::HeaderResult { token, album } => {
                if token != self.fetch_token {
                    return;
                }
                self.album = Some(album);
                self.header_done = true;
                self.maybe_finish();
            }
            AlbumDetailInput::HeaderFailed { token, error } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::warn!(album_id = self.album_id, error = %error, "album header fetch failed");
                self.header_done = true;
                self.maybe_finish();
            }
            AlbumDetailInput::TracksResult { token, items } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::info!(album_id = self.album_id, count = items.len(), "album tracks loaded");
                self.tracks = items;
                self.tracks_done = true;
                self.maybe_finish();
            }
            AlbumDetailInput::TracksFailed { token, error } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::warn!(album_id = self.album_id, error = %error, "album tracks fetch failed");
                self.state = ViewLoadState::Failed(error);
                self.tracks_done = true;
            }
            AlbumDetailInput::Play(track_id) => {
                let _ = sender.output(LibraryViewOutput::PlayTrack { track_id });
            }
            AlbumDetailInput::OpenArtist(id, name) => {
                let _ = sender.output(LibraryViewOutput::OpenArtist {
                    id: id.to_string(),
                    name,
                });
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
            self.album.as_ref(),
            &self.initial_title,
            sender.clone(),
        ));

        let list = ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["track-list"])
            .build();
        for (idx, track) in self.tracks.iter().enumerate() {
            let row = build_track_row(idx + 1, track, sender.clone());
            list.append(&row);
        }
        widgets.body.append(&list);
    }
}

impl AlbumDetailViewModel {
    fn maybe_finish(&mut self) {
        if self.header_done && self.tracks_done {
            // Header failure alone shouldn't trip Failed state — we still
            // have the track list to show. Failed is set explicitly by
            // TracksFailed.
            if !matches!(self.state, ViewLoadState::Failed(_)) {
                self.state = ViewLoadState::Loaded;
            }
        }
    }
}

fn clear_box(b: &GtkBox) {
    while let Some(child) = b.first_child() {
        b.remove(&child);
    }
}

fn build_header(
    album: Option<&Album>,
    initial_title: &str,
    sender: ComponentSender<AlbumDetailViewModel>,
) -> GtkBox {
    let header = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(20)
        .margin_bottom(8)
        .build();

    let cover = Image::builder()
        .icon_name("audio-x-generic-symbolic")
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

    let title = album
        .map(|a| {
            if a.name.is_empty() {
                initial_title.to_string()
            } else {
                a.name.clone()
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

    if let Some(a) = album {
        let artist = album_artist_name(a);
        if !artist.is_empty() {
            let artist_label = Label::builder()
                .label(&artist)
                .xalign(0.0)
                .css_classes(["title-3"])
                .build();
            // Click → navigate to artist detail.
            if let Some(art_id) = a.artist.as_ref().map(|x| x.id).filter(|id| *id > 0) {
                let click = gtk::GestureClick::new();
                let artist_owned = artist.clone();
                let sender_clone = sender.clone();
                click.connect_pressed(move |_, n_press, _, _| {
                    if n_press == 1 {
                        let _ = sender_clone.input_sender().send(
                            AlbumDetailInput::OpenArtist(art_id, artist_owned.clone()),
                        );
                    }
                });
                artist_label.add_controller(click);
            }
            info.append(&artist_label);
        }
        let mut bits: Vec<String> = Vec::new();
        if let Some(date) = a.release_date.as_deref().filter(|s| !s.is_empty()) {
            // Just show the year if it's a YYYY-MM-DD shape.
            let year = date.split('-').next().unwrap_or(date);
            bits.push(year.to_string());
        }
        if let Some(n) = a.num_tracks.filter(|n| *n > 0) {
            bits.push(format!("{n} tracks"));
        }
        if let Some(d) = a.duration.filter(|d| *d > 0) {
            bits.push(format_duration_minutes(d));
        }
        if !bits.is_empty() {
            let meta_label = Label::builder()
                .label(bits.join(" • "))
                .xalign(0.0)
                .css_classes(["dim-label"])
                .build();
            info.append(&meta_label);
        }
    }

    header.append(&info);
    header
}

/// `2h 03m` / `47m 12s` — coarser unit than the per-track `mm:ss`.
fn format_duration_minutes(seconds: i32) -> String {
    let s = seconds.max(0);
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let r = s % 60;
    if h > 0 {
        format!("{h}h {m:02}m")
    } else {
        format!("{m}m {r:02}s")
    }
}

fn build_track_row(
    idx: usize,
    track: &Track,
    sender: ComponentSender<AlbumDetailViewModel>,
) -> ListBoxRow {
    let row = ListBoxRow::builder()
        .css_classes(["track-row"])
        .activatable(true)
        .build();
    let body = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(12)
        .margin_end(12)
        .build();

    let index_label = Label::builder()
        .label(format!("{idx}"))
        .width_chars(4)
        .xalign(1.0)
        .css_classes(["dim-label", "monospace"])
        .build();
    body.append(&index_label);

    let title = if track.name.is_empty() {
        "Unknown"
    } else {
        track.name.as_str()
    };
    let title_label = Label::builder()
        .label(title)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .hexpand(true)
        .xalign(0.0)
        .build();
    body.append(&title_label);

    let artist_label = Label::builder()
        .label(track_artist_name(track))
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .width_chars(20)
        .xalign(0.0)
        .css_classes(["dim-label"])
        .build();
    body.append(&artist_label);

    let duration_label = Label::builder()
        .label(format_duration(track.duration))
        .width_chars(6)
        .xalign(1.0)
        .css_classes(["dim-label", "monospace"])
        .build();
    body.append(&duration_label);

    row.set_child(Some(&body));

    let track_id = track.id;
    row.connect_activate(move |_| {
        let _ = sender
            .input_sender()
            .send(AlbumDetailInput::Play(track_id));
    });

    row
}
