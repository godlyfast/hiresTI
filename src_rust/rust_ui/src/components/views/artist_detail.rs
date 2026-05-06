//! Artist detail page. Header (avatar + name + bio summary) plus two
//! sections: Top Tracks (a vertical track list) and Albums (a card flow).
//! Each section fetches independently — failure of one doesn't block the
//! others. The bio is best-effort: a missing/error response just hides
//! the bio paragraph.

use relm4::gtk::{
    self, prelude::*, Box as GtkBox, FlowBox, FlowBoxChild, Image, Label, ListBox, ListBoxRow,
    Orientation, ScrolledWindow,
};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use rust_tidal_core::api::{Album, Artist, Bio, Track};

use crate::components::views::common::{
    album_artist_name, build_error_widget, build_loading_widget, format_duration,
    track_artist_name, LibraryViewOutput, ViewLoadState,
};
use crate::services::tidal_session::{spawn_blocking, TidalSessionService};

pub struct ArtistDetailInit {
    pub session: TidalSessionService,
    pub artist_id: i64,
    pub initial_name: String,
}

pub struct ArtistDetailViewModel {
    session: TidalSessionService,
    artist_id: i64,
    initial_name: String,
    artist: Option<Artist>,
    bio: Option<Bio>,
    top_tracks: Vec<Track>,
    albums: Vec<Album>,
    state: ViewLoadState,
    fetch_token: u64,
    artist_done: bool,
    top_tracks_done: bool,
    albums_done: bool,
}

#[derive(Debug, Clone)]
pub enum ArtistDetailInput {
    Refresh,
    ArtistResult { token: u64, artist: Artist },
    ArtistFailed { token: u64, error: String },
    BioResult { token: u64, bio: Option<Bio> },
    TopTracksResult { token: u64, items: Vec<Track> },
    TopTracksFailed { token: u64, error: String },
    AlbumsResult { token: u64, items: Vec<Album> },
    AlbumsFailed { token: u64, error: String },
    Play(i64),
    OpenAlbum(i64, String),
}

pub struct ArtistDetailWidgets {
    root: ScrolledWindow,
    body: GtkBox,
}

impl SimpleComponent for ArtistDetailViewModel {
    type Init = ArtistDetailInit;
    type Input = ArtistDetailInput;
    type Output = LibraryViewOutput;
    type Root = ScrolledWindow;
    type Widgets = ArtistDetailWidgets;

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
            .spacing(20)
            .margin_top(16)
            .margin_bottom(16)
            .margin_start(16)
            .margin_end(16)
            .build();
        root.set_child(Some(&body));

        let model = Self {
            session: init.session,
            artist_id: init.artist_id,
            initial_name: init.initial_name,
            artist: None,
            bio: None,
            top_tracks: Vec::new(),
            albums: Vec::new(),
            state: ViewLoadState::Idle,
            fetch_token: 0,
            artist_done: false,
            top_tracks_done: false,
            albums_done: false,
        };
        let widgets = ArtistDetailWidgets {
            root: root.clone(),
            body,
        };
        sender
            .input_sender()
            .send(ArtistDetailInput::Refresh)
            .ok();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            ArtistDetailInput::Refresh => {
                if matches!(self.state, ViewLoadState::Loading) {
                    return;
                }
                self.state = ViewLoadState::Loading;
                self.artist_done = false;
                self.top_tracks_done = false;
                self.albums_done = false;
                self.fetch_token = self.fetch_token.wrapping_add(1);
                let token = self.fetch_token;
                let id = self.artist_id;

                let svc1 = self.session.clone();
                let s1 = sender.input_sender().clone();
                spawn_blocking(
                    move || svc1.fetch_artist_blocking(id),
                    move |result| {
                        let msg = match result {
                            Ok(artist) => ArtistDetailInput::ArtistResult { token, artist },
                            Err(e) => ArtistDetailInput::ArtistFailed {
                                token,
                                error: e.to_string(),
                            },
                        };
                        let _ = s1.send(msg);
                    },
                );

                // Bio is best-effort — TIDAL 404s for many artists. We
                // don't gate the page on it, so failures collapse to None.
                let svc_bio = self.session.clone();
                let s_bio = sender.input_sender().clone();
                spawn_blocking(
                    move || svc_bio.artist_bio_blocking(id),
                    move |result| {
                        let bio = result.ok();
                        let _ = s_bio.send(ArtistDetailInput::BioResult { token, bio });
                    },
                );

                let svc2 = self.session.clone();
                let s2 = sender.input_sender().clone();
                spawn_blocking(
                    move || svc2.artist_top_tracks_blocking(id),
                    move |result| {
                        let msg = match result {
                            Ok(items) => ArtistDetailInput::TopTracksResult { token, items },
                            Err(e) => ArtistDetailInput::TopTracksFailed {
                                token,
                                error: e.to_string(),
                            },
                        };
                        let _ = s2.send(msg);
                    },
                );

                let svc3 = self.session.clone();
                let s3 = sender.input_sender().clone();
                spawn_blocking(
                    move || svc3.artist_albums_blocking(id),
                    move |result| {
                        let msg = match result {
                            Ok(items) => ArtistDetailInput::AlbumsResult { token, items },
                            Err(e) => ArtistDetailInput::AlbumsFailed {
                                token,
                                error: e.to_string(),
                            },
                        };
                        let _ = s3.send(msg);
                    },
                );
            }
            ArtistDetailInput::ArtistResult { token, artist } => {
                if token != self.fetch_token {
                    return;
                }
                self.artist = Some(artist);
                self.artist_done = true;
                self.maybe_finish();
            }
            ArtistDetailInput::ArtistFailed { token, error } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::warn!(artist_id = self.artist_id, error = %error, "artist header fetch failed");
                self.artist_done = true;
                self.maybe_finish();
            }
            ArtistDetailInput::BioResult { token, bio } => {
                if token != self.fetch_token {
                    return;
                }
                self.bio = bio;
            }
            ArtistDetailInput::TopTracksResult { token, items } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::info!(artist_id = self.artist_id, count = items.len(), "artist top tracks loaded");
                self.top_tracks = items;
                self.top_tracks_done = true;
                self.maybe_finish();
            }
            ArtistDetailInput::TopTracksFailed { token, error } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::warn!(artist_id = self.artist_id, error = %error, "artist top tracks failed");
                self.top_tracks_done = true;
                self.maybe_finish();
            }
            ArtistDetailInput::AlbumsResult { token, items } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::info!(artist_id = self.artist_id, count = items.len(), "artist albums loaded");
                self.albums = items;
                self.albums_done = true;
                self.maybe_finish();
            }
            ArtistDetailInput::AlbumsFailed { token, error } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::warn!(artist_id = self.artist_id, error = %error, "artist albums failed");
                self.albums_done = true;
                self.maybe_finish();
            }
            ArtistDetailInput::Play(track_id) => {
                let _ = sender.output(LibraryViewOutput::PlayTrack { track_id });
            }
            ArtistDetailInput::OpenAlbum(id, title) => {
                let _ = sender.output(LibraryViewOutput::OpenAlbum {
                    id: id.to_string(),
                    title,
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
            self.artist.as_ref(),
            self.bio.as_ref(),
            &self.initial_name,
        ));

        if !self.top_tracks.is_empty() {
            widgets.body.append(&section_label("Top Tracks"));
            let list = ListBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .css_classes(["track-list"])
                .build();
            for (idx, track) in self.top_tracks.iter().enumerate() {
                let row = build_track_row(idx + 1, track, sender.clone());
                list.append(&row);
            }
            widgets.body.append(&list);
        }

        if !self.albums.is_empty() {
            widgets.body.append(&section_label("Albums"));
            let flow = FlowBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .max_children_per_line(8)
                .min_children_per_line(2)
                .row_spacing(12)
                .column_spacing(12)
                .homogeneous(true)
                .build();
            for album in &self.albums {
                let card = build_album_card(album, sender.clone());
                let child = FlowBoxChild::builder().child(&card).build();
                flow.append(&child);
            }
            widgets.body.append(&flow);
        }
    }
}

impl ArtistDetailViewModel {
    fn maybe_finish(&mut self) {
        if self.artist_done && self.top_tracks_done && self.albums_done {
            self.state = ViewLoadState::Loaded;
        }
    }
}

fn clear_box(b: &GtkBox) {
    while let Some(child) = b.first_child() {
        b.remove(&child);
    }
}

fn section_label(text: &str) -> Label {
    Label::builder()
        .label(text)
        .xalign(0.0)
        .css_classes(["title-3"])
        .margin_top(8)
        .build()
}

fn build_header(artist: Option<&Artist>, bio: Option<&Bio>, initial_name: &str) -> GtkBox {
    let header = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(20)
        .margin_bottom(4)
        .build();

    let avatar = Image::builder()
        .icon_name("avatar-default-symbolic")
        .pixel_size(180)
        .css_classes(["album-cover-img"])
        .build();
    header.append(&avatar);

    let info = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .vexpand(true)
        .valign(gtk::Align::Center)
        .build();

    let name = artist
        .map(|a| {
            if a.name.is_empty() {
                initial_name.to_string()
            } else {
                a.name.clone()
            }
        })
        .unwrap_or_else(|| initial_name.to_string());
    let name_label = Label::builder()
        .label(&name)
        .xalign(0.0)
        .css_classes(["title-1"])
        .wrap(true)
        .build();
    info.append(&name_label);

    if let Some(b) = bio.and_then(|b| b.text.as_deref().filter(|s| !s.is_empty())) {
        let bio_label = Label::builder()
            .label(strip_bio_markup(b))
            .xalign(0.0)
            .wrap(true)
            .max_width_chars(70)
            .lines(4)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["dim-label"])
            .build();
        info.append(&bio_label);
    }

    header.append(&info);
    header
}

/// Strip TIDAL's bio inline markup like `[wimpLink artistId="123"]Foo[/wimpLink]`
/// down to just the visible text. The bio surface is read-only here so we
/// don't need to preserve the cross-link metadata.
fn strip_bio_markup(s: &str) -> String {
    // Quick-and-dirty: drop any `[...]` sequence. Phase 7-D will swap this
    // for a rich-text widget that follows the wimpLink targets.
    let mut out = String::with_capacity(s.len());
    let mut depth = 0;
    for ch in s.chars() {
        match ch {
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

fn build_track_row(
    idx: usize,
    track: &Track,
    sender: ComponentSender<ArtistDetailViewModel>,
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
            .send(ArtistDetailInput::Play(track_id));
    });

    row
}

fn build_album_card(album: &Album, sender: ComponentSender<ArtistDetailViewModel>) -> GtkBox {
    let card = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .css_classes(["album-card"])
        .build();

    let cover = Image::builder()
        .icon_name("audio-x-generic-symbolic")
        .pixel_size(160)
        .css_classes(["album-cover-img"])
        .build();
    card.append(&cover);

    let title = if album.name.is_empty() {
        "Unknown"
    } else {
        album.name.as_str()
    };
    let title_label = Label::builder()
        .label(title)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(18)
        .xalign(0.0)
        .css_classes(["heading"])
        .build();
    card.append(&title_label);

    let artist = album_artist_name(album);
    if !artist.is_empty() {
        let artist_label = Label::builder()
            .label(&artist)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(18)
            .xalign(0.0)
            .css_classes(["dim-label", "caption"])
            .build();
        card.append(&artist_label);
    }

    let click = gtk::GestureClick::new();
    let id = album.id;
    let title_owned = title.to_string();
    click.connect_pressed(move |_, n_press, _, _| {
        if n_press == 1 {
            let _ = sender.input_sender().send(ArtistDetailInput::OpenAlbum(
                id,
                title_owned.clone(),
            ));
        }
    });
    card.add_controller(click);

    card
}
