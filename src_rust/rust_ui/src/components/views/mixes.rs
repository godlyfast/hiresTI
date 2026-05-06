//! Favorited Mixes (and Daily / My / Discovery mixes). Phase 5 only
//! shows the user's explicitly-favorited mixes; Phase 6 (Discovery
//! views) layers in the home-page mixes the API exposes elsewhere.

use relm4::gtk::{
    self, prelude::*, Box as GtkBox, FlowBox, FlowBoxChild, Image, Label, Orientation,
    ScrolledWindow,
};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use std::collections::HashMap;
use std::path::PathBuf;

use rust_tidal_core::api::Mix;

use crate::components::views::common::{
    build_empty_widget, build_error_widget, build_loading_widget, CoverPaths, LibraryViewOutput,
    ViewLoadState,
};
use crate::services::covers::fetch_covers_batch_blocking;
use crate::services::tidal_session::{spawn_blocking, TidalSessionService};

const COVER_SIZE: u32 = 160;

pub struct MixesViewModel {
    session: TidalSessionService,
    items: Vec<Mix>,
    state: ViewLoadState,
    fetch_token: u64,
    covers: CoverPaths,
}

#[derive(Debug, Clone)]
pub enum MixesViewInput {
    Refresh,
    FetchResult { token: u64, items: Vec<Mix> },
    FetchFailed { token: u64, error: String },
    CoversBatch { token: u64, covers: HashMap<String, PathBuf> },
    Open(String, String),
}

pub struct MixesViewWidgets {
    root: ScrolledWindow,
    flow: FlowBox,
}

impl SimpleComponent for MixesViewModel {
    type Init = TidalSessionService;
    type Input = MixesViewInput;
    type Output = LibraryViewOutput;
    type Root = ScrolledWindow;
    type Widgets = MixesViewWidgets;

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
        ComponentParts {
            model: Self {
                session: init,
                items: Vec::new(),
                state: ViewLoadState::Idle,
                fetch_token: 0,
                covers: HashMap::new(),
            },
            widgets: MixesViewWidgets {
                root: root.clone(),
                flow,
            },
        }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            MixesViewInput::Refresh => {
                if matches!(self.state, ViewLoadState::Loading) {
                    return;
                }
                self.state = ViewLoadState::Loading;
                self.fetch_token = self.fetch_token.wrapping_add(1);
                let token = self.fetch_token;
                let svc = self.session.clone();
                let sender_in = sender.input_sender().clone();
                spawn_blocking(
                    move || svc.list_favorite_mixes_blocking(),
                    move |result| {
                        let _ = sender_in.send(match result {
                            Ok(items) => MixesViewInput::FetchResult { token, items },
                            Err(e) => MixesViewInput::FetchFailed {
                                token,
                                error: e.to_string(),
                            },
                        });
                    },
                );
            }
            MixesViewInput::FetchResult { token, items } => {
                if token != self.fetch_token {
                    return;
                }
                let cover_ids: Vec<String> = items
                    .iter()
                    .filter_map(|m| m.image.clone().or_else(|| m.detail_image.clone()))
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
                            let _ = sender_in.send(MixesViewInput::CoversBatch {
                                token: cover_token,
                                covers,
                            });
                        },
                    );
                }
            }
            MixesViewInput::CoversBatch { token, covers } => {
                if token != self.fetch_token {
                    return;
                }
                self.covers.extend(covers);
            }
            MixesViewInput::FetchFailed { token, error } => {
                if token != self.fetch_token {
                    return;
                }
                self.state = ViewLoadState::Failed(error);
            }
            MixesViewInput::Open(id, title) => {
                let _ = sender.output(LibraryViewOutput::OpenMix { id, title });
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, sender: ComponentSender<Self>) {
        clear_flowbox(&widgets.flow);
        match &self.state {
            ViewLoadState::Idle => widgets.root.set_child(Some(&widgets.flow)),
            ViewLoadState::Loading => widgets.root.set_child(Some(&build_loading_widget())),
            ViewLoadState::Loaded => {
                widgets.root.set_child(Some(&widgets.flow));
                if self.items.is_empty() {
                    widgets.root.set_child(Some(&build_empty_widget(
                        "You haven't favorited any mixes yet.",
                    )));
                } else {
                    for mix in &self.items {
                        let path = mix
                            .image
                            .as_deref()
                            .or(mix.detail_image.as_deref())
                            .and_then(|id| self.covers.get(id).cloned());
                        let card = build_mix_card(mix, path, sender.clone());
                        let child = FlowBoxChild::builder().child(&card).build();
                        widgets.flow.append(&child);
                    }
                }
            }
            ViewLoadState::Failed(err) => widgets.root.set_child(Some(&build_error_widget(err))),
        }
    }
}

fn clear_flowbox(flow: &FlowBox) {
    while let Some(child) = flow.first_child() {
        flow.remove(&child);
    }
}

fn build_mix_card(
    mix: &Mix,
    cover_path: Option<PathBuf>,
    sender: ComponentSender<MixesViewModel>,
) -> GtkBox {
    let card = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .css_classes(["mix-card"])
        .build();
    let cover = Image::builder()
        .icon_name("audio-x-generic-symbolic")
        .pixel_size(160)
        .build();
    if let Some(p) = cover_path {
        cover.set_from_file(Some(&p));
    }
    card.append(&cover);
    let title = Label::builder()
        .label(if mix.title.is_empty() { "Untitled" } else { mix.title.as_str() })
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(18)
        .xalign(0.0)
        .css_classes(["heading"])
        .build();
    card.append(&title);
    if let Some(sub) = mix.sub_title.as_ref() {
        if !sub.is_empty() {
            let sub_label = Label::builder()
                .label(sub)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .max_width_chars(18)
                .xalign(0.0)
                .css_classes(["dim-label", "caption"])
                .build();
            card.append(&sub_label);
        }
    }

    let click = gtk::GestureClick::new();
    let id = mix.id.clone();
    let title_owned = mix.title.clone();
    click.connect_pressed(move |_, n_press, _, _| {
        if n_press == 1 {
            let _ = sender
                .input_sender()
                .send(MixesViewInput::Open(id.clone(), title_owned.clone()));
        }
    });
    card.add_controller(click);
    card
}
