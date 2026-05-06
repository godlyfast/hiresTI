//! Artists grid: same FlowBox pattern as Albums, with artist cards.

use relm4::gtk::{
    self, prelude::*, Box as GtkBox, FlowBox, FlowBoxChild, Image, Label, Orientation,
    ScrolledWindow,
};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use rust_tidal_core::api::Artist;

use crate::components::views::common::{
    build_empty_widget, build_error_widget, build_loading_widget, LibraryViewOutput,
    ViewLoadState,
};
use crate::services::tidal_session::{spawn_blocking, TidalSessionService};

pub struct ArtistsViewModel {
    session: TidalSessionService,
    items: Vec<Artist>,
    state: ViewLoadState,
    fetch_token: u64,
}

#[derive(Debug, Clone)]
pub enum ArtistsViewInput {
    Refresh,
    FetchResult { token: u64, items: Vec<Artist> },
    FetchFailed { token: u64, error: String },
    Open(String, String),
}

pub struct ArtistsViewWidgets {
    root: ScrolledWindow,
    flow: FlowBox,
}

impl SimpleComponent for ArtistsViewModel {
    type Init = TidalSessionService;
    type Input = ArtistsViewInput;
    type Output = LibraryViewOutput;
    type Root = ScrolledWindow;
    type Widgets = ArtistsViewWidgets;

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
            },
            widgets: ArtistsViewWidgets {
                root: root.clone(),
                flow,
            },
        }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            ArtistsViewInput::Refresh => {
                if matches!(self.state, ViewLoadState::Loading) {
                    return;
                }
                self.state = ViewLoadState::Loading;
                self.fetch_token = self.fetch_token.wrapping_add(1);
                let token = self.fetch_token;
                let svc = self.session.clone();
                let sender_in = sender.input_sender().clone();
                spawn_blocking(
                    move || svc.list_favorite_artists_blocking(),
                    move |result| {
                        let _ = sender_in.send(match result {
                            Ok(items) => ArtistsViewInput::FetchResult { token, items },
                            Err(e) => ArtistsViewInput::FetchFailed {
                                token,
                                error: e.to_string(),
                            },
                        });
                    },
                );
            }
            ArtistsViewInput::FetchResult { token, items } => {
                if token != self.fetch_token {
                    return;
                }
                self.items = items;
                self.state = ViewLoadState::Loaded;
            }
            ArtistsViewInput::FetchFailed { token, error } => {
                if token != self.fetch_token {
                    return;
                }
                self.state = ViewLoadState::Failed(error);
            }
            ArtistsViewInput::Open(id, name) => {
                let _ = sender.output(LibraryViewOutput::OpenArtist { id, name });
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, sender: ComponentSender<Self>) {
        clear_flowbox(&widgets.flow);
        match &self.state {
            ViewLoadState::Idle => {
                widgets.root.set_child(Some(&widgets.flow));
            }
            ViewLoadState::Loading => widgets.root.set_child(Some(&build_loading_widget())),
            ViewLoadState::Loaded => {
                widgets.root.set_child(Some(&widgets.flow));
                if self.items.is_empty() {
                    widgets.root.set_child(Some(&build_empty_widget(
                        "You haven't favorited any artists yet.",
                    )));
                } else {
                    for artist in &self.items {
                        let card = build_artist_card(artist, sender.clone());
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

fn build_artist_card(artist: &Artist, sender: ComponentSender<ArtistsViewModel>) -> GtkBox {
    let card = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .css_classes(["artist-card"])
        .build();
    let cover = Image::builder()
        .icon_name("avatar-default-symbolic")
        .pixel_size(160)
        .css_classes(["circular-avatar"])
        .build();
    card.append(&cover);
    let name_label = Label::builder()
        .label(if artist.name.is_empty() {
            "Unknown"
        } else {
            artist.name.as_str()
        })
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(18)
        .xalign(0.5)
        .css_classes(["heading"])
        .build();
    card.append(&name_label);

    let click = gtk::GestureClick::new();
    let id = artist.id.to_string();
    let name_owned = artist.name.clone();
    click.connect_pressed(move |_, n_press, _, _| {
        if n_press == 1 {
            let _ = sender
                .input_sender()
                .send(ArtistsViewInput::Open(id.clone(), name_owned.clone()));
        }
    });
    card.add_controller(click);
    card
}
