//! Root Relm4 component. Phase 1: empty `Adw.ApplicationWindow` with the
//! correct title, default size from settings, and a placeholder body.
//!
//! Phase 3 will replace the placeholder with the actual sidebar + content
//! stack + mini player layout. The split is intentional: by keeping this
//! file's surface area tiny in Phase 1 we exercise the build/run/window-
//! lifecycle plumbing in isolation, before any view code lands.

use libadwaita::prelude::*;
use libadwaita::{ApplicationWindow, HeaderBar};
use relm4::adw::Application;
use relm4::gtk::{self, Box as GtkBox, Label};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use crate::messages::AppInput;
use crate::model::AppModel;
use crate::settings::Settings;

#[relm4::component(pub)]
impl SimpleComponent for AppModel {
    type Init = AppModel;
    type Input = AppInput;
    type Output = ();

    view! {
        ApplicationWindow {
            set_title: Some("HiresTI"),
            set_default_width: model.settings.window_width,
            set_default_height: model.settings.window_height,

            #[wrap(Some)]
            set_content = &GtkBox {
                set_orientation: gtk::Orientation::Vertical,

                HeaderBar {
                    set_title_widget: Some(&gtk::Label::new(Some("HiresTI"))),
                },

                Label {
                    set_label: "Phase 1 shell — content panes will land in Phase 3.",
                    set_vexpand: true,
                    add_css_class: "dim-label",
                },
            },
        },
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = init;
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            AppInput::NavigateTo(target) => {
                self.current_nav = target;
                self.settings.last_nav = target.as_id().into();
                if let Err(e) = self.settings.save() {
                    tracing::warn!(error = %e, "failed to persist settings.last_nav");
                }
            }
            AppInput::WindowResized { width, height } => {
                if self.settings.remember_window_size {
                    self.settings.window_width = width;
                    self.settings.window_height = height;
                }
            }
            AppInput::ApplySettings(new) => {
                self.settings = new;
                if let Err(e) = self.settings.save() {
                    tracing::warn!(error = %e, "failed to persist applied settings");
                }
            }
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
    runner.run::<AppModel>(AppModel::from_settings(initial));
    0
}
