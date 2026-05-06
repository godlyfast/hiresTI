//! Liked Tracks list. Vertical ListBox with one row per track:
//! `<index>  <title>          <artist>  <duration>`. Click → play.

use relm4::gtk::{self, prelude::*, ListBox, ScrolledWindow};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use rust_tidal_core::api::Track;

use crate::components::views::common::{
    build_empty_widget, build_error_widget, build_loading_widget, build_track_row,
    LibraryViewOutput, ViewLoadState,
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
    /// Row index inside `items` — handler builds the queue context.
    Play(usize),
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
            TracksViewInput::Play(idx) => {
                if idx < self.items.len() {
                    let _ = sender.output(LibraryViewOutput::PlayContext {
                        tracks: self.items.clone(),
                        start_index: idx,
                        source: crate::state::playback::PlaybackSource::LikedTracks,
                    });
                }
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
                        let s = sender.clone();
                        let row = build_track_row(idx + 1, track, move || {
                            let _ = s.input_sender().send(TracksViewInput::Play(idx));
                        });
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

