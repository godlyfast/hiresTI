//! Root Relm4 component. Owns the `AppModel` and composes the four
//! top-level region components (header, sidebar, content stack, mini
//! player) via `Controller<T>`. Each child emits an `Output` enum that
//! a small `forward()` adapter translates into `AppInput`.
//!
//! Phase 3 deliverable: the app boots, the sidebar shows three sections
//! with 13 navigation rows, clicking a row switches the content stack
//! and persists `settings.last_nav`. Header search / login / settings
//! menu / mini player buttons are wired to outputs the root logs but
//! doesn't yet act on (Phase 4+ work).

use libadwaita::prelude::*;
use libadwaita::{ApplicationWindow, ToolbarView};
use relm4::adw::Application;
use relm4::gtk::{Box as GtkBox, Orientation, Paned, Separator};
use relm4::{
    Component, ComponentController, ComponentParts, ComponentSender, Controller,
    SimpleComponent,
};

use crate::components::content_stack::{ContentStackInput, ContentStackModel};
use crate::components::header::{HeaderModel, HeaderOutput};
use crate::components::mini_player::{MiniPlayerModel, MiniPlayerOutput};
use crate::components::sidebar::{SidebarInput, SidebarModel, SidebarOutput};
use crate::messages::AppInput;
use crate::model::AppModel;
use crate::settings::Settings;

pub struct AppController {
    model: AppModel,
    // Header / mini hold their controllers alive so the widgets stay
    // mounted; we don't yet send messages back into them in Phase 3,
    // but Phase 4+ will (auth display name → header, transport state →
    // mini). Suppress dead-code until then.
    #[allow(dead_code)]
    header: Controller<HeaderModel>,
    sidebar: Controller<SidebarModel>,
    content: Controller<ContentStackModel>,
    #[allow(dead_code)]
    mini: Controller<MiniPlayerModel>,
}

pub struct AppWidgets {
    window: ApplicationWindow,
}

impl SimpleComponent for AppController {
    type Init = AppModel;
    type Input = AppInput;
    type Output = ();
    type Root = ApplicationWindow;
    type Widgets = AppWidgets;

    fn init_root() -> Self::Root {
        ApplicationWindow::builder()
            .title("HiresTI")
            .default_width(1250)
            .default_height(800)
            .build()
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // Apply window geometry from persisted settings.
        root.set_default_size(init.settings.window_width, init.settings.window_height);

        // ---- Children -------------------------------------------------
        let header = HeaderModel::builder().launch(()).forward(
            sender.input_sender(),
            |out| match out {
                HeaderOutput::Search(q) => AppInput::Search(q),
                HeaderOutput::LoginRequested => AppInput::RequestLogin,
                HeaderOutput::OpenSettings => AppInput::OpenSettings,
                HeaderOutput::OpenAbout => AppInput::OpenAbout,
            },
        );

        let sidebar = SidebarModel::builder()
            .launch(init.current_nav)
            .forward(sender.input_sender(), |out| match out {
                SidebarOutput::Navigate(t) => AppInput::NavigateTo(t),
            });

        let content = ContentStackModel::builder()
            .launch(init.current_nav)
            .detach();

        let mini = MiniPlayerModel::builder().launch(()).forward(
            sender.input_sender(),
            |out| match out {
                MiniPlayerOutput::Play => AppInput::TransportPlay,
                MiniPlayerOutput::Pause => AppInput::TransportPause,
                MiniPlayerOutput::Next => AppInput::TransportNext,
                MiniPlayerOutput::Previous => AppInput::TransportPrev,
                MiniPlayerOutput::Seek(p) => AppInput::TransportSeek(p),
            },
        );

        // ---- Layout ---------------------------------------------------
        // Adw.ApplicationWindow content: ToolbarView with the header
        // bar at the top, a horizontal Paned (sidebar | content) in the
        // middle, and the mini player as a bottom bar.
        let toolbar = ToolbarView::new();
        toolbar.add_top_bar(header.widget());

        let body = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .build();

        let paned = Paned::builder()
            .orientation(Orientation::Horizontal)
            .resize_start_child(false)
            .shrink_start_child(false)
            .position(220)
            .build();
        paned.set_start_child(Some(sidebar.widget()));
        paned.set_end_child(Some(content.widget()));
        body.append(&paned);

        body.append(&Separator::new(Orientation::Horizontal));
        body.append(mini.widget());

        toolbar.set_content(Some(&body));
        root.set_content(Some(&toolbar));

        // Track window size so settings can persist on close.
        let s = sender.clone();
        root.connect_default_width_notify(move |w| {
            let _ = s.input_sender().send(AppInput::WindowResized {
                width: w.default_width(),
                height: w.default_height(),
            });
        });
        let s = sender.clone();
        root.connect_default_height_notify(move |w| {
            let _ = s.input_sender().send(AppInput::WindowResized {
                width: w.default_width(),
                height: w.default_height(),
            });
        });

        let model = Self {
            model: init,
            header,
            sidebar,
            content,
            mini,
        };
        let widgets = AppWidgets { window: root };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            AppInput::NavigateTo(target) => {
                self.model.current_nav = target;
                self.model.settings.last_nav = target.as_id().into();
                self.persist();
                self.sidebar
                    .sender()
                    .send(SidebarInput::SetActive(target))
                    .ok();
                self.content
                    .sender()
                    .send(ContentStackInput::Show(target))
                    .ok();
            }
            AppInput::WindowResized { width, height } => {
                if self.model.settings.remember_window_size {
                    self.model.settings.window_width = width;
                    self.model.settings.window_height = height;
                    // Don't persist on every pixel of drag — settings save
                    // happens on close (Phase 4 / on settings.save() call).
                }
            }
            AppInput::ApplySettings(new) => {
                self.model.settings = new;
                self.persist();
            }
            AppInput::Search(q) => {
                tracing::info!(query = %q, "search submitted (Phase 7 will route this)");
            }
            AppInput::RequestLogin => {
                tracing::info!("login requested (Phase 4 will open the auth flow)");
            }
            AppInput::OpenSettings => {
                tracing::info!("settings dialog requested (Phase 8)");
            }
            AppInput::OpenAbout => {
                tracing::info!("about dialog requested (Phase 8)");
            }
            AppInput::TransportPlay
            | AppInput::TransportPause
            | AppInput::TransportNext
            | AppInput::TransportPrev => {
                tracing::info!(?msg, "transport (Phase 4 wires the audio engine)");
            }
            AppInput::TransportSeek(p) => {
                tracing::debug!(seek = p, "seek (Phase 4 wires the audio engine)");
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        // Touch the window field so the borrow checker keeps it live;
        // future view sync (e.g. window-title binding to current track)
        // wires here.
        let _ = &widgets.window;
    }
}

impl AppController {
    fn persist(&self) {
        if let Err(e) = self.model.settings.save() {
            tracing::warn!(error = %e, "settings save failed");
        }
    }
}

/// Build the `Adw.Application`, wire the root component, and run the
/// GTK main loop. Called from `main()` once logging + paths are set up.
pub fn run(initial: Settings) -> i32 {
    let app = Application::builder()
        .application_id("com.hiresti.player")
        .build();

    let runner = relm4::RelmApp::from_app(app);
    runner.run::<AppController>(AppModel::from_settings(initial));
    0
}
