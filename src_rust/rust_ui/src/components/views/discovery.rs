//! Shared discovery page view. Renders a vertical scroll of category
//! sections — each section title + a horizontal flow of cards/rows —
//! by walking the Page returned by rust_tidal_core's pages parser.
//! Used by Home, New, Top, and Hi-Res. Genres / Decades / Moods use
//! the tabbed flavor in `tabbed_discovery.rs`.

use std::rc::Rc;

use relm4::gtk::{self, Box as GtkBox, Orientation, ScrolledWindow, Separator};
use relm4::gtk::prelude::*;
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use std::collections::HashMap;
use std::path::PathBuf;

use rust_tidal_core::api::Page;

use crate::components::views::common::{
    build_empty_widget, build_error_widget, build_loading_widget, build_page_category_section,
    page_item_cover_id, CoverPaths, LibraryViewOutput, ViewLoadState,
};
use crate::services::covers::fetch_covers_batch_blocking;
use crate::services::tidal_session::{spawn_blocking, TidalSessionService};

/// Pixel size used for grid cards. Matches the placeholder Image size
/// so the swap doesn't reflow the layout.
const COVER_SIZE: u32 = 160;

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
    covers: CoverPaths,
}

#[derive(Debug, Clone)]
pub enum DiscoveryViewInput {
    Refresh,
    FetchResult { token: u64, page: Page },
    FetchFailed { token: u64, error: String },
    /// Background batch finished. Token guards against stale results.
    CoversBatch { token: u64, covers: HashMap<String, PathBuf> },
    /// Card click from the rendering helpers — forwarded straight to
    /// the parent via the component's Output sender.
    Forward(LibraryViewOutput),
}

pub struct DiscoveryViewWidgets {
    root: ScrolledWindow,
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
            covers: HashMap::new(),
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
                // Collect the cover-ids for this page and kick a batch
                // fetch so update_view's render can swap placeholders
                // for real images on the next pass.
                let cover_ids: Vec<String> = page
                    .categories
                    .iter()
                    .flat_map(|c| c.items.iter())
                    .filter_map(page_item_cover_id)
                    .collect();
                self.page = Some(page);
                self.state = ViewLoadState::Loaded;
                if !cover_ids.is_empty() {
                    let cover_token = self.fetch_token;
                    let sender_in = sender.input_sender().clone();
                    spawn_blocking(
                        move || fetch_covers_batch_blocking(cover_ids, COVER_SIZE),
                        move |covers| {
                            let _ = sender_in.send(DiscoveryViewInput::CoversBatch {
                                token: cover_token,
                                covers,
                            });
                        },
                    );
                }
            }
            DiscoveryViewInput::CoversBatch { token, covers } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::info!(count = covers.len(), "discovery covers batch loaded");
                self.covers.extend(covers);
            }
            DiscoveryViewInput::FetchFailed { token, error } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::warn!(error = %error, "discovery page fetch failed");
                self.state = ViewLoadState::Failed(error);
            }
            DiscoveryViewInput::Forward(out) => {
                let _ = sender.output(out);
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
                    let s = sender.clone();
                    let opener: crate::components::views::common::CategoryOpener =
                        Rc::new(move |out| {
                            let _ = s.input_sender().send(DiscoveryViewInput::Forward(out));
                        });
                    for (i, category) in categories.iter().enumerate() {
                        if i > 0 {
                            widgets
                                .body
                                .append(&Separator::new(Orientation::Horizontal));
                        }
                        let section =
                            build_page_category_section(category, opener.clone(), &self.covers);
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
