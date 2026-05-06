//! Device-code OAuth login dialog. Shows the user the verification URL
//! and 4-letter user code TIDAL returned and waits for the parent to
//! drive the polling loop.
//!
//! No widget here owns the polling timer — that lives in `AppController`
//! (see Phase 4 wiring). The dialog is a passive view: parent feeds it
//! the `DeviceLogin` payload to display, and a status string for the
//! "polling…" / "almost done" / "expired" state.

use libadwaita::prelude::*;
use libadwaita::Window;
use relm4::gtk::{self, Box as GtkBox, Button, Label, Orientation};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use rust_tidal_core::api::DeviceLogin;

pub struct LoginDialogModel {
    info: Option<DeviceLogin>,
    status: String,
}

#[derive(Debug, Clone)]
pub enum LoginDialogInput {
    /// Render a fresh device-code payload.
    Show(DeviceLogin),
    /// Update the status line under the code (e.g. "Waiting for
    /// authorization…", "Almost done", "Code expired").
    SetStatus(String),
    /// Hide the dialog (parent calls this on success, error, or cancel).
    Hide,
}

#[derive(Debug, Clone)]
pub enum LoginDialogOutput {
    /// User closed the dialog or pressed Cancel — parent should abort
    /// the poll loop.
    Cancelled,
    /// User pressed "Open in Browser" — parent should xdg-open the
    /// verification URI. Implementation in app.rs to keep this
    /// component free of subprocess noise.
    OpenInBrowser(String),
    /// User pressed "Copy Code" — parent should put `code` on the
    /// clipboard. Same separation reason.
    CopyCode(String),
    /// User picked "Use browser sign-in (PKCE)" — parent kicks the
    /// PKCE login dialog flow.
    UsePkce,
}

pub struct LoginDialogWidgets {
    window: Window,
    code_label: Label,
    url_label: Label,
    status_label: Label,
}

impl SimpleComponent for LoginDialogModel {
    type Init = ();
    type Input = LoginDialogInput;
    type Output = LoginDialogOutput;
    type Root = Window;
    type Widgets = LoginDialogWidgets;

    fn init_root() -> Self::Root {
        Window::builder()
            .modal(true)
            .resizable(false)
            .default_width(420)
            .default_height(280)
            .title("Sign in to TIDAL")
            .hide_on_close(true)
            .build()
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let body = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(12)
            .margin_top(20)
            .margin_bottom(20)
            .margin_start(28)
            .margin_end(28)
            .build();

        let title = Label::builder()
            .label("Sign in to TIDAL")
            .css_classes(["title-2"])
            .xalign(0.0)
            .build();
        body.append(&title);

        let intro = Label::builder()
            .label("Open the URL below in any browser, sign in, and enter the code:")
            .wrap(true)
            .xalign(0.0)
            .css_classes(["dim-label"])
            .build();
        body.append(&intro);

        let url_row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .build();
        let url_label = Label::builder()
            .label("link.tidal.com/…")
            .selectable(true)
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["monospace"])
            .build();
        url_row.append(&url_label);
        let open_btn = Button::builder()
            .label("Open in Browser")
            .css_classes(["suggested-action"])
            .build();
        let s = sender.clone();
        let url_label_clone = url_label.clone();
        open_btn.connect_clicked(move |_| {
            let url = url_label_clone.text().to_string();
            let _ = s.output(LoginDialogOutput::OpenInBrowser(url));
        });
        url_row.append(&open_btn);
        body.append(&url_row);

        let code_row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .build();
        let code_label = Label::builder()
            .label("------")
            .selectable(true)
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["title-1", "monospace"])
            .build();
        code_row.append(&code_label);
        let copy_btn = Button::builder().label("Copy").build();
        let s = sender.clone();
        let code_label_clone = code_label.clone();
        copy_btn.connect_clicked(move |_| {
            let code = code_label_clone.text().to_string();
            let _ = s.output(LoginDialogOutput::CopyCode(code));
        });
        code_row.append(&copy_btn);
        body.append(&code_row);

        let status_label = Label::builder()
            .label("Waiting for authorization…")
            .xalign(0.0)
            .css_classes(["dim-label"])
            .build();
        body.append(&status_label);

        let action_row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .halign(gtk::Align::End)
            .build();
        let pkce_btn = Button::builder()
            .label("Use browser sign-in")
            .css_classes(["flat"])
            .build();
        let s = sender.clone();
        pkce_btn.connect_clicked(move |_| {
            let _ = s.output(LoginDialogOutput::UsePkce);
        });
        action_row.append(&pkce_btn);
        let cancel_btn = Button::builder().label("Cancel").build();
        let s = sender.clone();
        cancel_btn.connect_clicked(move |_| {
            let _ = s.output(LoginDialogOutput::Cancelled);
        });
        action_row.append(&cancel_btn);
        body.append(&action_row);

        // Same on close-via-window-X.
        let s = sender.clone();
        root.connect_close_request(move |_| {
            let _ = s.output(LoginDialogOutput::Cancelled);
            glib::Propagation::Proceed
        });

        root.set_content(Some(&body));

        let model = Self {
            info: None,
            status: "Waiting for authorization…".into(),
        };
        let widgets = LoginDialogWidgets {
            window: root,
            code_label,
            url_label,
            status_label,
        };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            LoginDialogInput::Show(info) => {
                self.info = Some(info);
                self.status = "Waiting for authorization…".into();
            }
            LoginDialogInput::SetStatus(s) => {
                self.status = s;
            }
            LoginDialogInput::Hide => {
                self.info = None;
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        match &self.info {
            Some(info) => {
                widgets.code_label.set_label(&info.user_code);
                let url = if !info.verification_uri_complete.is_empty() {
                    &info.verification_uri_complete
                } else {
                    &info.verification_uri
                };
                widgets.url_label.set_label(url);
                widgets.status_label.set_label(&self.status);
                if !widgets.window.is_visible() {
                    widgets.window.present();
                }
            }
            None => {
                if widgets.window.is_visible() {
                    widgets.window.set_visible(false);
                }
            }
        }
    }
}

use relm4::gtk::glib;
