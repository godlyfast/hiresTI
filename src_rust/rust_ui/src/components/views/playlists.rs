//! User Playlists list. Folder navigation is intentionally deferred to
//! Phase 7 (detail/edit views) — Phase 5 just shows the flat top-level
//! playlist set.

use relm4::gtk::{
    self, prelude::*, Box as GtkBox, Image, Label, ListBox, ListBoxRow, Orientation,
    ScrolledWindow,
};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use std::collections::HashMap;
use std::path::PathBuf;

use rust_tidal_core::api::Playlist;

use crate::components::views::common::{
    build_empty_widget, build_error_widget, build_loading_widget, CoverPaths, LibraryViewOutput,
    ViewLoadState,
};
use crate::services::covers::fetch_covers_batch_blocking;
use crate::services::tidal_session::{spawn_blocking, TidalSessionService};

/// Smaller cache bucket — playlists render in a vertical list with
/// ~48px row icons, so 80x80 downscales cleanly.
const COVER_SIZE: u32 = 80;

pub struct PlaylistsViewModel {
    session: TidalSessionService,
    items: Vec<Playlist>,
    state: ViewLoadState,
    fetch_token: u64,
    covers: CoverPaths,
}

#[derive(Debug, Clone)]
pub enum PlaylistsViewInput {
    Refresh,
    FetchResult { token: u64, items: Vec<Playlist> },
    FetchFailed { token: u64, error: String },
    CoversBatch { token: u64, covers: HashMap<String, PathBuf> },
    Open(String, String),
}

pub struct PlaylistsViewWidgets {
    root: ScrolledWindow,
    list: ListBox,
}

impl SimpleComponent for PlaylistsViewModel {
    type Init = TidalSessionService;
    type Input = PlaylistsViewInput;
    type Output = LibraryViewOutput;
    type Root = ScrolledWindow;
    type Widgets = PlaylistsViewWidgets;

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
            .css_classes(["playlist-list"])
            .build();
        root.set_child(Some(&list));
        ComponentParts {
            model: Self {
                session: init,
                items: Vec::new(),
                state: ViewLoadState::Idle,
                fetch_token: 0,
                covers: HashMap::new(),
            },
            widgets: PlaylistsViewWidgets {
                root: root.clone(),
                list,
            },
        }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            PlaylistsViewInput::Refresh => {
                if matches!(self.state, ViewLoadState::Loading) {
                    return;
                }
                self.state = ViewLoadState::Loading;
                self.fetch_token = self.fetch_token.wrapping_add(1);
                let token = self.fetch_token;
                let svc = self.session.clone();
                let sender_in = sender.input_sender().clone();
                spawn_blocking(
                    move || svc.list_user_playlists_blocking(),
                    move |result| {
                        let _ = sender_in.send(match result {
                            Ok(items) => PlaylistsViewInput::FetchResult { token, items },
                            Err(e) => PlaylistsViewInput::FetchFailed {
                                token,
                                error: e.to_string(),
                            },
                        });
                    },
                );
            }
            PlaylistsViewInput::FetchResult { token, items } => {
                if token != self.fetch_token {
                    return;
                }
                let cover_ids: Vec<String> = items
                    .iter()
                    .filter_map(|p| {
                        p.square_image.clone().or_else(|| p.image.clone())
                    })
                    .filter(|id| !self.covers.contains_key(id))
                    .collect();
                self.items = items;
                self.state = ViewLoadState::Loaded;
                if !cover_ids.is_empty() {
                    let cover_token = self.fetch_token;
                    let sender_in = sender.input_sender().clone();
                    spawn_blocking(
                        move || fetch_covers_batch_blocking(cover_ids, COVER_SIZE),
                        move |covers| {
                            let _ = sender_in.send(PlaylistsViewInput::CoversBatch {
                                token: cover_token,
                                covers,
                            });
                        },
                    );
                }
            }
            PlaylistsViewInput::CoversBatch { token, covers } => {
                if token != self.fetch_token {
                    return;
                }
                self.covers.extend(covers);
            }
            PlaylistsViewInput::FetchFailed { token, error } => {
                if token != self.fetch_token {
                    return;
                }
                self.state = ViewLoadState::Failed(error);
            }
            PlaylistsViewInput::Open(uuid, title) => {
                let _ = sender.output(LibraryViewOutput::OpenPlaylist { uuid, title });
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, sender: ComponentSender<Self>) {
        clear_listbox(&widgets.list);
        match &self.state {
            ViewLoadState::Idle => widgets.root.set_child(Some(&widgets.list)),
            ViewLoadState::Loading => widgets.root.set_child(Some(&build_loading_widget())),
            ViewLoadState::Loaded => {
                widgets.root.set_child(Some(&widgets.list));
                if self.items.is_empty() {
                    widgets.root.set_child(Some(&build_empty_widget(
                        "You haven't created any playlists yet.",
                    )));
                } else {
                    for pl in &self.items {
                        let path = pl
                            .square_image
                            .as_deref()
                            .or(pl.image.as_deref())
                            .and_then(|id| self.covers.get(id).cloned());
                        widgets
                            .list
                            .append(&build_playlist_row(pl, path, sender.clone()));
                    }
                }
            }
            ViewLoadState::Failed(err) => widgets.root.set_child(Some(&build_error_widget(err))),
        }
    }
}

fn clear_listbox(list: &ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

fn build_playlist_row(
    pl: &Playlist,
    cover_path: Option<PathBuf>,
    sender: ComponentSender<PlaylistsViewModel>,
) -> ListBoxRow {
    let row = ListBoxRow::builder()
        .css_classes(["playlist-row"])
        .activatable(true)
        .build();
    let body = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(12)
        .margin_end(12)
        .build();

    let cover = Image::builder()
        .icon_name("audio-x-generic-symbolic")
        .pixel_size(48)
        .build();
    if let Some(p) = cover_path {
        cover.set_from_file(Some(&p));
    }
    body.append(&cover);

    let info = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .hexpand(true)
        .build();
    let title = Label::builder()
        .label(if pl.name.is_empty() { "Untitled" } else { pl.name.as_str() })
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .xalign(0.0)
        .css_classes(["heading"])
        .build();
    info.append(&title);
    let count_text = pl
        .num_tracks
        .map(|n| format!("{n} tracks"))
        .unwrap_or_default();
    let subtitle = Label::builder()
        .label(count_text)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .xalign(0.0)
        .css_classes(["dim-label", "caption"])
        .build();
    info.append(&subtitle);
    body.append(&info);

    row.set_child(Some(&body));

    let uuid = pl.id.clone();
    let title_owned = pl.name.clone();
    row.connect_activate(move |_| {
        let _ = sender
            .input_sender()
            .send(PlaylistsViewInput::Open(uuid.clone(), title_owned.clone()));
    });

    row
}
