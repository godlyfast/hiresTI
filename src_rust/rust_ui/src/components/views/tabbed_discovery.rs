//! Tabbed discovery surface used by Genres / Decades / Moods. The top
//! row holds one button per tab; clicking a tab fetches that tab's
//! `/pages/<api_path>` lazily and renders its categories below.
//!
//! Three modes:
//! - `TabSource::Definitions(path)`: tabs come from another `/pages/X`
//!   page whose items each carry `(title, apiPath)` (e.g. genre_page,
//!   moods_page). The tab list is fetched on Refresh.
//! - `TabSource::Static(Vec<(label, path)>)`: tabs are hardcoded —
//!   used for Decades since the seven decade pages have stable URLs.
//!
//! Per-tab content is cached so re-clicking a tab re-renders without a
//! re-fetch; Refresh wipes the cache and reloads.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use relm4::gtk::prelude::*;
use relm4::gtk::{
    self, Box as GtkBox, Button, Label, Orientation, ScrolledWindow, Separator, ToggleButton,
};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use rust_tidal_core::api::Page;

use crate::components::views::common::{
    build_empty_widget, build_error_widget, build_loading_widget, build_page_category_section,
    page_item_cover_id, CoverPaths, LibraryViewOutput, ViewLoadState,
};
use crate::services::covers::fetch_covers_batch_blocking;
use crate::services::tidal_session::{spawn_blocking, TidalSessionService};

const COVER_SIZE: u32 = 160;

#[derive(Debug, Clone)]
pub enum TabSource {
    /// Genres / Moods: fetch tab definitions from `<path>` whose items
    /// each carry a `(title, apiPath)` pair. The path is the discovery
    /// page that surfaces the tab list (`pages/genre_page`,
    /// `pages/moods_page`).
    Definitions(String),
    /// Decades: tabs are hardcoded `(label, path)` pairs.
    Static(Vec<(String, String)>),
}

pub struct TabbedDiscoveryInit {
    pub session: TidalSessionService,
    pub source: TabSource,
    /// Empty-state copy when the source produces zero tabs.
    pub empty_message: &'static str,
}

#[derive(Debug, Clone)]
struct TabDef {
    label: String,
    path: String,
}

pub struct TabbedDiscoveryViewModel {
    session: TidalSessionService,
    source: TabSource,
    empty_message: &'static str,
    tabs: Vec<TabDef>,
    active: Option<usize>,
    /// Cached page content keyed by tab index. Hit on re-click — only
    /// Refresh wipes the cache.
    cache: HashMap<usize, Page>,
    state: ViewLoadState,
    /// Bumped on every Refresh + every tab switch so a slow tab fetch
    /// can't clobber a fresher selection.
    fetch_token: u64,
    /// Cover-art lookup shared across all tabs in this view. Filled
    /// progressively as each tab's batch fetch completes.
    covers: CoverPaths,
}

#[derive(Debug, Clone)]
pub enum TabbedDiscoveryInput {
    Refresh,
    TabsLoaded {
        token: u64,
        tabs: Vec<(String, String)>,
    },
    TabsFailed {
        token: u64,
        error: String,
    },
    SelectTab(usize),
    TabContentLoaded {
        token: u64,
        tab_index: usize,
        page: Page,
    },
    TabContentFailed {
        token: u64,
        tab_index: usize,
        error: String,
    },
    CoversBatch {
        token: u64,
        covers: HashMap<String, PathBuf>,
    },
    Forward(LibraryViewOutput),
}

pub struct TabbedDiscoveryWidgets {
    root: GtkBox,
    /// Tab button row at the top.
    tab_bar: GtkBox,
    /// Content scroll area below the tab bar.
    content: ScrolledWindow,
}

impl SimpleComponent for TabbedDiscoveryViewModel {
    type Init = TabbedDiscoveryInit;
    type Input = TabbedDiscoveryInput;
    type Output = LibraryViewOutput;
    type Root = GtkBox;
    type Widgets = TabbedDiscoveryWidgets;

    fn init_root() -> Self::Root {
        GtkBox::builder()
            .orientation(Orientation::Vertical)
            .vexpand(true)
            .hexpand(true)
            .build()
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let tab_bar = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .margin_top(12)
            .margin_bottom(8)
            .margin_start(16)
            .margin_end(16)
            .build();
        root.append(&tab_bar);
        root.append(&Separator::new(Orientation::Horizontal));

        let content = ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .hexpand(true)
            .build();
        root.append(&content);

        let model = Self {
            session: init.session,
            source: init.source,
            empty_message: init.empty_message,
            tabs: Vec::new(),
            active: None,
            cache: HashMap::new(),
            state: ViewLoadState::Idle,
            fetch_token: 0,
            covers: HashMap::new(),
        };
        let widgets = TabbedDiscoveryWidgets {
            root: root.clone(),
            tab_bar,
            content,
        };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            TabbedDiscoveryInput::Refresh => {
                // Re-navigation back to this view shouldn't dump the
                // cache. Only first-load + explicit failure-retry hit
                // the network.
                if !self.tabs.is_empty()
                    && !matches!(self.state, ViewLoadState::Idle | ViewLoadState::Failed(_))
                {
                    return;
                }
                self.cache.clear();
                self.tabs.clear();
                self.active = None;
                self.fetch_token = self.fetch_token.wrapping_add(1);
                let token = self.fetch_token;
                self.state = ViewLoadState::Loading;
                match self.source.clone() {
                    TabSource::Static(tabs) => {
                        let _ = sender
                            .input_sender()
                            .send(TabbedDiscoveryInput::TabsLoaded { token, tabs });
                    }
                    TabSource::Definitions(path) => {
                        let svc = self.session.clone();
                        let sender_in = sender.input_sender().clone();
                        spawn_blocking(
                            move || svc.fetch_tab_definitions_blocking(&path),
                            move |result| {
                                let msg = match result {
                                    Ok(tabs) => TabbedDiscoveryInput::TabsLoaded { token, tabs },
                                    Err(e) => TabbedDiscoveryInput::TabsFailed {
                                        token,
                                        error: e.to_string(),
                                    },
                                };
                                let _ = sender_in.send(msg);
                            },
                        );
                    }
                }
            }
            TabbedDiscoveryInput::TabsLoaded { token, tabs } => {
                if token != self.fetch_token {
                    return;
                }
                self.tabs = tabs
                    .into_iter()
                    .map(|(label, path)| TabDef { label, path })
                    .collect();
                if self.tabs.is_empty() {
                    self.state = ViewLoadState::Loaded;
                } else {
                    let _ = sender
                        .input_sender()
                        .send(TabbedDiscoveryInput::SelectTab(0));
                }
            }
            TabbedDiscoveryInput::TabsFailed { token, error } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::warn!(error = %error, "tab definitions fetch failed");
                self.state = ViewLoadState::Failed(error);
            }
            TabbedDiscoveryInput::SelectTab(idx) => {
                let Some(tab) = self.tabs.get(idx) else {
                    return;
                };
                self.active = Some(idx);
                if self.cache.contains_key(&idx) {
                    self.state = ViewLoadState::Loaded;
                    return;
                }
                self.state = ViewLoadState::Loading;
                self.fetch_token = self.fetch_token.wrapping_add(1);
                let token = self.fetch_token;
                let svc = self.session.clone();
                let path = tab.path.clone();
                let sender_in = sender.input_sender().clone();
                spawn_blocking(
                    move || svc.fetch_discovery_page_blocking(&path),
                    move |result| {
                        let msg = match result {
                            Ok(page) => TabbedDiscoveryInput::TabContentLoaded {
                                token,
                                tab_index: idx,
                                page,
                            },
                            Err(e) => TabbedDiscoveryInput::TabContentFailed {
                                token,
                                tab_index: idx,
                                error: e.to_string(),
                            },
                        };
                        let _ = sender_in.send(msg);
                    },
                );
            }
            TabbedDiscoveryInput::TabContentLoaded {
                token,
                tab_index,
                page,
            } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::info!(
                    tab_index,
                    categories = page.categories.len(),
                    "tab content loaded"
                );
                let cover_ids: Vec<String> = page
                    .categories
                    .iter()
                    .flat_map(|c| c.items.iter())
                    .filter_map(page_item_cover_id)
                    .filter(|id| !self.covers.contains_key(id))
                    .collect();
                self.cache.insert(tab_index, page);
                self.state = ViewLoadState::Loaded;
                if !cover_ids.is_empty() {
                    let cover_token = self.fetch_token;
                    let sender_in = sender.input_sender().clone();
                    spawn_blocking(
                        move || fetch_covers_batch_blocking(cover_ids, COVER_SIZE),
                        move |covers| {
                            let _ = sender_in.send(TabbedDiscoveryInput::CoversBatch {
                                token: cover_token,
                                covers,
                            });
                        },
                    );
                }
            }
            TabbedDiscoveryInput::CoversBatch { token, covers } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::info!(count = covers.len(), "tabbed covers batch loaded");
                self.covers.extend(covers);
            }
            TabbedDiscoveryInput::TabContentFailed {
                token,
                tab_index,
                error,
            } => {
                if token != self.fetch_token {
                    return;
                }
                tracing::warn!(tab_index, error = %error, "tab content fetch failed");
                self.state = ViewLoadState::Failed(error);
            }
            TabbedDiscoveryInput::Forward(out) => {
                let _ = sender.output(out);
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, sender: ComponentSender<Self>) {
        // Rebuild the tab bar each tick so active styling stays in sync.
        // Tab counts are small (<30) so the cost is negligible.
        clear_box(&widgets.tab_bar);
        for (i, tab) in self.tabs.iter().enumerate() {
            let btn = ToggleButton::builder()
                .label(&tab.label)
                .active(Some(i) == self.active)
                .build();
            let s = sender.clone();
            btn.connect_clicked(move |b| {
                if b.is_active() {
                    let _ = s.input_sender().send(TabbedDiscoveryInput::SelectTab(i));
                }
            });
            widgets.tab_bar.append(&btn);
        }

        match &self.state {
            ViewLoadState::Idle => {
                widgets.content.set_child(None::<&Button>);
            }
            ViewLoadState::Loading => {
                widgets.content.set_child(Some(&build_loading_widget()));
            }
            ViewLoadState::Failed(err) => {
                widgets.content.set_child(Some(&build_error_widget(err)));
            }
            ViewLoadState::Loaded => {
                if self.tabs.is_empty() {
                    widgets
                        .content
                        .set_child(Some(&build_empty_widget(self.empty_message)));
                    return;
                }
                let body = GtkBox::builder()
                    .orientation(Orientation::Vertical)
                    .spacing(20)
                    .margin_top(16)
                    .margin_bottom(16)
                    .margin_start(16)
                    .margin_end(16)
                    .build();
                let active = self.active.unwrap_or(0);
                let categories = self
                    .cache
                    .get(&active)
                    .map(|p| p.categories.as_slice())
                    .unwrap_or(&[]);
                if categories.is_empty() {
                    let lbl = Label::builder()
                        .label("(this tab returned no sections)")
                        .xalign(0.0)
                        .css_classes(["dim-label"])
                        .build();
                    body.append(&lbl);
                } else {
                    let s = sender.clone();
                    let opener: crate::components::views::common::CategoryOpener =
                        Rc::new(move |out| {
                            let _ = s.input_sender().send(TabbedDiscoveryInput::Forward(out));
                        });
                    for (i, category) in categories.iter().enumerate() {
                        if i > 0 {
                            body.append(&Separator::new(Orientation::Horizontal));
                        }
                        let section =
                            build_page_category_section(category, opener.clone(), &self.covers);
                        body.append(&section);
                    }
                }
                widgets.content.set_child(Some(&body));
            }
        }
        let _ = &widgets.root;
    }
}

fn clear_box(b: &GtkBox) {
    while let Some(child) = b.first_child() {
        b.remove(&child);
    }
}
