//! Albums grid: fetches the user's favorite albums and renders them in
//! a FlowBox. Phase 5 deliverable: list reads from
//! `TidalSessionService::list_favorite_albums_blocking`. Image loading
//! is stubbed with a placeholder icon — Phase 7 wires the resources.tidal.com
//! cover fetcher.

use relm4::gtk::{
    self, prelude::*, Box as GtkBox, FlowBox, FlowBoxChild, Image, Label, Orientation,
    ScrolledWindow,
};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use rust_tidal_core::api::Album;

use crate::components::views::common::{
    build_empty_widget, build_error_widget, build_loading_widget, LibraryViewOutput,
    ViewLoadState,
};
use crate::services::tidal_session::{spawn_blocking, TidalSessionService};

pub struct AlbumsViewModel {
    session: TidalSessionService,
    items: Vec<Album>,
    state: ViewLoadState,
    /// Bumped on every Refresh so a stale in-flight fetch can recognize
    /// it should drop its result instead of clobbering newer state.
    fetch_token: u64,
}

#[derive(Debug, Clone)]
pub enum AlbumsViewInput {
    /// Sent by the parent on first navigation (or on a manual reload
    /// gesture). Fires off a blocking fetch on a worker thread.
    Refresh,
    /// Worker thread reports the fetch result. The token must match
    /// `fetch_token` or the result is discarded as stale.
    FetchResult { token: u64, items: Vec<Album> },
    /// Worker thread reports failure with the error string.
    FetchFailed { token: u64, error: String },
    /// User clicked an album card. The parent forwards this as a
    /// detail-view nav.
    Open(String, String),
}

pub struct AlbumsViewWidgets {
    root: ScrolledWindow,
    flow: FlowBox,
}

impl SimpleComponent for AlbumsViewModel {
    type Init = TidalSessionService;
    type Input = AlbumsViewInput;
    type Output = LibraryViewOutput;
    type Root = ScrolledWindow;
    type Widgets = AlbumsViewWidgets;

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
        let flow = FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .max_children_per_line(8)
            .min_children_per_line(2)
            .row_spacing(16)
            .column_spacing(16)
            .margin_top(16)
            .margin_bottom(16)
            .margin_start(16)
            .margin_end(16)
            .homogeneous(true)
            .build();
        root.set_child(Some(&flow));

        // Initial placeholder so the empty-but-not-yet-loaded state
        // doesn't look broken.
        let placeholder = Label::builder()
            .label("")
            .build();
        flow.append(&placeholder);

        let model = Self {
            session: init,
            items: Vec::new(),
            state: ViewLoadState::Idle,
            fetch_token: 0,
        };
        let widgets = AlbumsViewWidgets {
            root: root.clone(),
            flow,
        };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            AlbumsViewInput::Refresh => {
                if matches!(self.state, ViewLoadState::Loading) {
                    return;
                }
                self.state = ViewLoadState::Loading;
                self.fetch_token = self.fetch_token.wrapping_add(1);
                let token = self.fetch_token;
                let svc = self.session.clone();
                let sender_in = sender.input_sender().clone();
                spawn_blocking(
                    move || svc.list_favorite_albums_blocking(),
                    move |result| {
                        let msg = match result {
                            Ok(items) => AlbumsViewInput::FetchResult { token, items },
                            Err(e) => AlbumsViewInput::FetchFailed {
                                token,
                                error: e.to_string(),
                            },
                        };
                        let _ = sender_in.send(msg);
                    },
                );
            }
            AlbumsViewInput::FetchResult { token, items } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::info!(count = items.len(), "albums loaded");
                self.items = items;
                self.state = ViewLoadState::Loaded;
            }
            AlbumsViewInput::FetchFailed { token, error } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::warn!(error = %error, "albums fetch failed");
                self.state = ViewLoadState::Failed(error);
            }
            AlbumsViewInput::Open(id, title) => {
                let _ = sender.output(LibraryViewOutput::OpenAlbum { id, title });
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, sender: ComponentSender<Self>) {
        // Replace the entire FlowBox children based on current state.
        // Phase 5's volume of items (typically <500 favorites) makes
        // full rebuild cheap enough; FactoryVecDeque comes in Phase 7
        // for the larger detail-view track lists.
        clear_flowbox(&widgets.flow);

        match &self.state {
            ViewLoadState::Idle => {
                widgets.root.set_child(Some(&widgets.flow));
                // No placeholder — the parent typically dispatches Refresh
                // before the page is ever shown.
            }
            ViewLoadState::Loading => {
                widgets.root.set_child(Some(&build_loading_widget()));
            }
            ViewLoadState::Loaded => {
                widgets.root.set_child(Some(&widgets.flow));
                if self.items.is_empty() {
                    widgets.root.set_child(Some(&build_empty_widget(
                        "You haven't favorited any albums yet.",
                    )));
                } else {
                    for album in &self.items {
                        let card = build_album_card(album, sender.clone());
                        let child = FlowBoxChild::builder().child(&card).build();
                        widgets.flow.append(&child);
                    }
                }
            }
            ViewLoadState::Failed(err) => {
                widgets.root.set_child(Some(&build_error_widget(err)));
            }
        }
    }
}

fn clear_flowbox(flow: &FlowBox) {
    while let Some(child) = flow.first_child() {
        flow.remove(&child);
    }
}

fn build_album_card(album: &Album, sender: ComponentSender<AlbumsViewModel>) -> GtkBox {
    let card = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .css_classes(["album-card"])
        .build();

    // Cover slot. Phase 7 swaps the placeholder icon for a downloaded
    // resources.tidal.com cover via the existing cache pattern.
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

    let artist_label = Label::builder()
        .label(primary_artist_name(album))
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(18)
        .xalign(0.0)
        .css_classes(["dim-label", "caption"])
        .build();
    card.append(&artist_label);

    // Click → emit Open with id + title.
    let click = gtk::GestureClick::new();
    let id = album.id.to_string();
    let title_owned = title.to_string();
    click.connect_pressed(move |_, n_press, _, _| {
        if n_press == 1 {
            let _ = sender
                .input_sender()
                .send(AlbumsViewInput::Open(id.clone(), title_owned.clone()));
        }
    });
    card.add_controller(click);

    card
}

fn primary_artist_name(album: &Album) -> String {
    if let Some(a) = album.artist.as_ref() {
        if !a.name.is_empty() {
            return a.name.clone();
        }
    }
    let names: Vec<&str> = album
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
