//! Liked Tracks list. Vertical ListBox with one row per track:
//! `<index>  <title>          <artist>  <duration>`. Click → play.

use relm4::gtk::{
    self, prelude::*, Box as GtkBox, Label, ListBox, ListBoxRow, Orientation, ScrolledWindow,
};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use rust_tidal_core::api::Track;

use crate::components::views::common::{
    build_empty_widget, build_error_widget, build_loading_widget, LibraryViewOutput,
    ViewLoadState,
};
use crate::services::tidal_session::{spawn_blocking, TidalSessionService};

pub struct TracksViewModel {
    session: TidalSessionService,
    items: Vec<Track>,
    state: ViewLoadState,
    fetch_token: u64,
}

#[derive(Debug, Clone)]
pub enum TracksViewInput {
    Refresh,
    FetchResult { token: u64, items: Vec<Track> },
    FetchFailed { token: u64, error: String },
    Play(i64),
}

pub struct TracksViewWidgets {
    root: ScrolledWindow,
    list: ListBox,
}

impl SimpleComponent for TracksViewModel {
    type Init = TidalSessionService;
    type Input = TracksViewInput;
    type Output = LibraryViewOutput;
    type Root = ScrolledWindow;
    type Widgets = TracksViewWidgets;

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
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let list = ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["track-list"])
            .build();
        root.set_child(Some(&list));

        let model = Self {
            session: init,
            items: Vec::new(),
            state: ViewLoadState::Idle,
            fetch_token: 0,
        };
        let widgets = TracksViewWidgets {
            root: root.clone(),
            list,
        };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            TracksViewInput::Refresh => {
                if matches!(self.state, ViewLoadState::Loading) {
                    return;
                }
                self.state = ViewLoadState::Loading;
                self.fetch_token = self.fetch_token.wrapping_add(1);
                let token = self.fetch_token;
                let svc = self.session.clone();
                let sender_in = sender.input_sender().clone();
                spawn_blocking(
                    move || svc.list_favorite_tracks_blocking(),
                    move |result| {
                        let msg = match result {
                            Ok(items) => TracksViewInput::FetchResult { token, items },
                            Err(e) => TracksViewInput::FetchFailed {
                                token,
                                error: e.to_string(),
                            },
                        };
                        let _ = sender_in.send(msg);
                    },
                );
            }
            TracksViewInput::FetchResult { token, items } => {
                if token != self.fetch_token {
                    return;
                }
                self.items = items;
                self.state = ViewLoadState::Loaded;
            }
            TracksViewInput::FetchFailed { token, error } => {
                if token != self.fetch_token {
                    return;
                }
                self.state = ViewLoadState::Failed(error);
            }
            TracksViewInput::Play(track_id) => {
                let _ = sender.output(LibraryViewOutput::PlayTrack { track_id });
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, sender: ComponentSender<Self>) {
        clear_listbox(&widgets.list);
        match &self.state {
            ViewLoadState::Idle => {
                widgets.root.set_child(Some(&widgets.list));
            }
            ViewLoadState::Loading => {
                widgets.root.set_child(Some(&build_loading_widget()));
            }
            ViewLoadState::Loaded => {
                widgets.root.set_child(Some(&widgets.list));
                if self.items.is_empty() {
                    widgets.root.set_child(Some(&build_empty_widget(
                        "You haven't liked any tracks yet.",
                    )));
                } else {
                    for (idx, track) in self.items.iter().enumerate() {
                        let row = build_track_row(idx + 1, track, sender.clone());
                        widgets.list.append(&row);
                    }
                }
            }
            ViewLoadState::Failed(err) => {
                widgets.root.set_child(Some(&build_error_widget(err)));
            }
        }
    }
}

fn clear_listbox(list: &ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

fn build_track_row(idx: usize, track: &Track, sender: ComponentSender<TracksViewModel>) -> ListBoxRow {
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
        let _ = sender.input_sender().send(TracksViewInput::Play(track_id));
    });

    row
}

fn track_artist_name(track: &Track) -> String {
    if let Some(a) = track.artist.as_ref() {
        if !a.name.is_empty() {
            return a.name.clone();
        }
    }
    let names: Vec<&str> = track
        .artists
        .iter()
        .map(|a| a.name.as_str())
        .filter(|s| !s.is_empty())
        .collect();
    if !names.is_empty() {
        names.join(", ")
    } else {
        String::new()
    }
}

fn format_duration(seconds: i32) -> String {
    let s = seconds.max(0);
    let m = s / 60;
    let r = s % 60;
    format!("{m}:{r:02}")
}
