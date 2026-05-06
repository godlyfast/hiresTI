//! Shared discovery page view. Renders a vertical scroll of category
//! sections — each section title + a horizontal flow of cards/rows —
//! by walking the Page returned by rust_tidal_core's pages parser.
//! Used by Home, New, Top, and Hi-Res. Genres / Decades / Moods are
//! tab-style and share enough structure to drop in here later, but
//! Phase 6 leaves them as placeholders.

use relm4::gtk::{
    self, prelude::*, Box as GtkBox, FlowBox, FlowBoxChild, Image, Label, Orientation,
    ScrolledWindow, Separator,
};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use rust_tidal_core::api::{Page, PageCategory, PageItem};

use crate::components::views::common::{
    build_empty_widget, build_error_widget, build_loading_widget, LibraryViewOutput,
    ViewLoadState,
};
use crate::services::tidal_session::{spawn_blocking, TidalSessionService};

/// What page this view should fetch. `Home` is the v2 `home/feed/static`
/// surface; `Path(p)` hits `/pages/<p>` directly.
#[derive(Debug, Clone)]
pub enum DiscoverySource {
    Home,
    Path(String),
}

pub struct DiscoveryViewInit {
    pub session: TidalSessionService,
    pub source: DiscoverySource,
    /// Empty-state copy when the fetch returns zero categories.
    pub empty_message: &'static str,
}

pub struct DiscoveryViewModel {
    session: TidalSessionService,
    source: DiscoverySource,
    empty_message: &'static str,
    page: Option<Page>,
    state: ViewLoadState,
    fetch_token: u64,
}

#[derive(Debug, Clone)]
pub enum DiscoveryViewInput {
    Refresh,
    FetchResult { token: u64, page: Page },
    FetchFailed { token: u64, error: String },
    OpenAlbum { id: i64, title: String },
    OpenArtist { id: i64, name: String },
    OpenPlaylist { id: String, title: String },
    OpenMix { id: String, title: String },
    PlayTrack { id: i64 },
}

pub struct DiscoveryViewWidgets {
    root: ScrolledWindow,
    /// The vertical Box that holds each rendered category section. Lives
    /// inside `root` when the view is in Loaded state.
    body: GtkBox,
}

impl SimpleComponent for DiscoveryViewModel {
    type Init = DiscoveryViewInit;
    type Input = DiscoveryViewInput;
    type Output = LibraryViewOutput;
    type Root = ScrolledWindow;
    type Widgets = DiscoveryViewWidgets;

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
            source: init.source,
            empty_message: init.empty_message,
            page: None,
            state: ViewLoadState::Idle,
            fetch_token: 0,
        };
        let widgets = DiscoveryViewWidgets {
            root: root.clone(),
            body,
        };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            DiscoveryViewInput::Refresh => {
                if matches!(self.state, ViewLoadState::Loading) {
                    return;
                }
                self.state = ViewLoadState::Loading;
                self.fetch_token = self.fetch_token.wrapping_add(1);
                let token = self.fetch_token;
                let svc = self.session.clone();
                let source = self.source.clone();
                let sender_in = sender.input_sender().clone();
                spawn_blocking(
                    move || match source {
                        DiscoverySource::Home => svc.fetch_home_page_blocking(),
                        DiscoverySource::Path(p) => svc.fetch_discovery_page_blocking(&p),
                    },
                    move |result| {
                        let msg = match result {
                            Ok(page) => DiscoveryViewInput::FetchResult { token, page },
                            Err(e) => DiscoveryViewInput::FetchFailed {
                                token,
                                error: e.to_string(),
                            },
                        };
                        let _ = sender_in.send(msg);
                    },
                );
            }
            DiscoveryViewInput::FetchResult { token, page } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::info!(
                    categories = page.categories.len(),
                    title = page.title.as_deref().unwrap_or(""),
                    "discovery page loaded"
                );
                self.page = Some(page);
                self.state = ViewLoadState::Loaded;
            }
            DiscoveryViewInput::FetchFailed { token, error } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::warn!(error = %error, "discovery page fetch failed");
                self.state = ViewLoadState::Failed(error);
            }
            DiscoveryViewInput::OpenAlbum { id, title } => {
                let _ = sender.output(LibraryViewOutput::OpenAlbum {
                    id: id.to_string(),
                    title,
                });
            }
            DiscoveryViewInput::OpenArtist { id, name } => {
                let _ = sender.output(LibraryViewOutput::OpenArtist {
                    id: id.to_string(),
                    name,
                });
            }
            DiscoveryViewInput::OpenPlaylist { id, title } => {
                let _ = sender.output(LibraryViewOutput::OpenPlaylist { uuid: id, title });
            }
            DiscoveryViewInput::OpenMix { id, title } => {
                let _ = sender.output(LibraryViewOutput::OpenMix { id, title });
            }
            DiscoveryViewInput::PlayTrack { id } => {
                let _ = sender.output(LibraryViewOutput::PlayTrack { track_id: id });
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, sender: ComponentSender<Self>) {
        clear_box(&widgets.body);

        match &self.state {
            ViewLoadState::Idle => {
                widgets.root.set_child(Some(&widgets.body));
            }
            ViewLoadState::Loading => {
                widgets.root.set_child(Some(&build_loading_widget()));
            }
            ViewLoadState::Loaded => {
                widgets.root.set_child(Some(&widgets.body));
                let categories = self
                    .page
                    .as_ref()
                    .map(|p| p.categories.as_slice())
                    .unwrap_or(&[]);
                if categories.is_empty() {
                    widgets
                        .root
                        .set_child(Some(&build_empty_widget(self.empty_message)));
                } else {
                    for (i, category) in categories.iter().enumerate() {
                        if i > 0 {
                            widgets
                                .body
                                .append(&Separator::new(Orientation::Horizontal));
                        }
                        let section = build_category_section(category, sender.clone());
                        widgets.body.append(&section);
                    }
                }
            }
            ViewLoadState::Failed(err) => {
                widgets.root.set_child(Some(&build_error_widget(err)));
            }
        }
    }
}

fn clear_box(b: &GtkBox) {
    while let Some(child) = b.first_child() {
        b.remove(&child);
    }
}

fn build_category_section(
    category: &PageCategory,
    sender: ComponentSender<DiscoveryViewModel>,
) -> GtkBox {
    let section = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .build();

    if let Some(title) = category.title.as_deref().filter(|s| !s.is_empty()) {
        let title_label = Label::builder()
            .label(title)
            .xalign(0.0)
            .css_classes(["title-3"])
            .build();
        section.append(&title_label);
    }
    if let Some(subtitle) = category.subtitle.as_deref().filter(|s| !s.is_empty()) {
        let sub = Label::builder()
            .label(subtitle)
            .xalign(0.0)
            .css_classes(["dim-label", "caption"])
            .build();
        section.append(&sub);
    }

    if category.items.is_empty() {
        let empty = Label::builder()
            .label("(no items)")
            .xalign(0.0)
            .css_classes(["dim-label"])
            .build();
        section.append(&empty);
        return section;
    }

    let flow = FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(8)
        .min_children_per_line(2)
        .row_spacing(12)
        .column_spacing(12)
        .homogeneous(true)
        .build();
    for item in &category.items {
        if let Some(card) = build_item_card(item, sender.clone()) {
            let child = FlowBoxChild::builder().child(&card).build();
            flow.append(&child);
        }
    }
    section.append(&flow);
    section
}

fn build_item_card(
    item: &PageItem,
    sender: ComponentSender<DiscoveryViewModel>,
) -> Option<GtkBox> {
    let card = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .css_classes(["album-card"])
        .build();

    // Phase 7 wires the cover fetcher; for now every card uses the same
    // placeholder icon so the layout is consistent.
    let cover = Image::builder()
        .icon_name("audio-x-generic-symbolic")
        .pixel_size(160)
        .css_classes(["album-cover-img"])
        .build();
    card.append(&cover);

    let (primary, secondary, on_click): (String, String, Box<dyn Fn(&ComponentSender<DiscoveryViewModel>)>) =
        match item {
            PageItem::Track(t) => {
                let title = if t.name.is_empty() {
                    "Unknown".to_string()
                } else {
                    t.name.clone()
                };
                let artist = primary_artist(t.artist.as_ref(), &t.artists);
                let id = t.id;
                (
                    title,
                    artist,
                    Box::new(move |s| {
                        let _ = s.input_sender().send(DiscoveryViewInput::PlayTrack { id });
                    }),
                )
            }
            PageItem::Album(a) => {
                let title = if a.name.is_empty() {
                    "Unknown".to_string()
                } else {
                    a.name.clone()
                };
                let artist = primary_artist(a.artist.as_ref(), &a.artists);
                let id = a.id;
                let title_owned = title.clone();
                (
                    title,
                    artist,
                    Box::new(move |s| {
                        let _ = s.input_sender().send(DiscoveryViewInput::OpenAlbum {
                            id,
                            title: title_owned.clone(),
                        });
                    }),
                )
            }
            PageItem::Artist(a) => {
                let name = if a.name.is_empty() {
                    "Unknown".to_string()
                } else {
                    a.name.clone()
                };
                let id = a.id;
                let name_owned = name.clone();
                (name, String::new(), Box::new(move |s| {
                    let _ = s.input_sender().send(DiscoveryViewInput::OpenArtist {
                        id,
                        name: name_owned.clone(),
                    });
                }))
            }
            PageItem::Playlist(p) => {
                let title = if p.name.is_empty() {
                    "Untitled playlist".to_string()
                } else {
                    p.name.clone()
                };
                let count = p
                    .num_tracks
                    .filter(|n| *n > 0)
                    .map(|n| format!("{n} tracks"))
                    .unwrap_or_default();
                let id = p.id.clone();
                let title_owned = title.clone();
                (
                    title,
                    count,
                    Box::new(move |s| {
                        let _ = s.input_sender().send(DiscoveryViewInput::OpenPlaylist {
                            id: id.clone(),
                            title: title_owned.clone(),
                        });
                    }),
                )
            }
            PageItem::Mix(m) => {
                let title = if m.title.is_empty() {
                    "Mix".to_string()
                } else {
                    m.title.clone()
                };
                let sub = m.sub_title.clone().unwrap_or_default();
                let id = m.id.clone();
                let title_owned = title.clone();
                (
                    title,
                    sub,
                    Box::new(move |s| {
                        let _ = s.input_sender().send(DiscoveryViewInput::OpenMix {
                            id: id.clone(),
                            title: title_owned.clone(),
                        });
                    }),
                )
            }
            PageItem::Video(_) => {
                // Phase 6 doesn't render videos — the discovery views skip
                // them. Returning None drops the card entirely.
                return None;
            }
            PageItem::Card(c) => {
                let title = c
                    .header
                    .clone()
                    .or_else(|| c.short_header.clone())
                    .or_else(|| c.title.clone())
                    .unwrap_or_else(|| "—".to_string());
                let sub = c
                    .short_sub_header
                    .clone()
                    .or_else(|| c.sub_title.clone())
                    .unwrap_or_default();
                // Cards without a typed action don't open anything in
                // Phase 6 — Phase 7's link router will pick them up.
                (title, sub, Box::new(|_| {}))
            }
        };

    let title_label = Label::builder()
        .label(&primary)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(18)
        .xalign(0.0)
        .css_classes(["heading"])
        .build();
    card.append(&title_label);

    if !secondary.is_empty() {
        let sub_label = Label::builder()
            .label(&secondary)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(18)
            .xalign(0.0)
            .css_classes(["dim-label", "caption"])
            .build();
        card.append(&sub_label);
    }

    let click = gtk::GestureClick::new();
    click.connect_pressed(move |_, n_press, _, _| {
        if n_press == 1 {
            on_click(&sender);
        }
    });
    card.add_controller(click);

    Some(card)
}

fn primary_artist(
    primary: Option<&rust_tidal_core::api::ArtistRef>,
    fallback: &[rust_tidal_core::api::ArtistRef],
) -> String {
    if let Some(a) = primary {
        if !a.name.is_empty() {
            return a.name.clone();
        }
    }
    let names: Vec<&str> = fallback
        .iter()
        .map(|a| a.name.as_str())
        .filter(|s| !s.is_empty())
        .collect();
    if names.is_empty() {
        String::new()
    } else {
        names.join(", ")
    }
}
