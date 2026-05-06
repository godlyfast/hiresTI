//! PKCE browser login dialog. Alternative to the device-code OAuth
//! flow that opens the authorize URL in the system browser, lets the
//! user sign in, then accepts the post-redirect URL pasted back from
//! the browser address bar. We don't ship a WebKit-embedded dialog —
//! that pulls a 50MB dependency for a flow that completes in two
//! clicks via the user's existing browser.
//!
//! This is a passive view: parent fetches the authorize URL on a
//! worker thread and feeds it in via `Show(url)`; user submitting
//! the redirect URL emits `Submit(url)` for the parent to call
//! `pkce_finish_blocking` on a worker thread.

use libadwaita::prelude::*;
use libadwaita::Window;
use relm4::gtk::{self, Box as GtkBox, Button, Entry, Label, Orientation};
use relm4::gtk::glib;
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

pub struct PkceLoginDialogModel {
    auth_url: Option<String>,
    status: String,
    busy: bool,
}

#[derive(Debug, Clone)]
pub enum PkceLoginDialogInput {
    /// Authorize URL came back from the worker; render it + present.
    Show(String),
    /// Status line ("Waiting for redirect URL", "Validating…",
    /// "Failed: …").
    SetStatus(String),
    /// Disable input while a finish call is in flight.
    SetBusy(bool),
    /// Hide the dialog (parent calls on success/error/cancel).
    Hide,
}

#[derive(Debug, Clone)]
pub enum PkceLoginDialogOutput {
    Cancelled,
    OpenInBrowser(String),
    Submit(String),
}

pub struct PkceLoginDialogWidgets {
    window: Window,
    url_label: Label,
    redirect_entry: Entry,
    status_label: Label,
    submit_btn: Button,
}

impl SimpleComponent for PkceLoginDialogModel {
    type Init = ();
    type Input = PkceLoginDialogInput;
    type Output = PkceLoginDialogOutput;
    type Root = Window;
    type Widgets = PkceLoginDialogWidgets;

    fn init_root() -> Self::Root {
        Window::builder()
            .modal(true)
            .resizable(false)
            .default_width(560)
            .default_height(360)
            .title("Browser sign-in")
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

        body.append(
            &Label::builder()
                .label("Sign in via browser (PKCE)")
                .css_classes(["title-2"])
                .xalign(0.0)
                .build(),
        );
        body.append(
            &Label::builder()
                .label(
                    "1. Open the URL below in your browser.\n\
                     2. Sign in to TIDAL.\n\
                     3. After redirecting, copy the FULL URL from \
                     your browser's address bar back into the field \
                     below and click Sign In.",
                )
                .wrap(true)
                .xalign(0.0)
                .css_classes(["dim-label"])
                .build(),
        );

        let url_row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .build();
        let url_label = Label::builder()
            .label("Loading authorize URL…")
            .selectable(true)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["monospace", "caption"])
            .build();
        url_row.append(&url_label);
        let open_btn = Button::builder()
            .label("Open in Browser")
            .css_classes(["suggested-action"])
            .build();
        let s = sender.clone();
        let url_label_for_open = url_label.clone();
        open_btn.connect_clicked(move |_| {
            let url = url_label_for_open.text().to_string();
            if !url.is_empty() && !url.starts_with("Loading") {
                let _ = s.output(PkceLoginDialogOutput::OpenInBrowser(url));
            }
        });
        url_row.append(&open_btn);
        body.append(&url_row);

        let redirect_entry = Entry::builder()
            .placeholder_text("Paste the redirect URL here")
            .build();
        body.append(&redirect_entry);

        let action_row = GtkBox::builder()
            .orientation(Orientation::Horizontal)
            .spacing(8)
            .halign(gtk::Align::End)
            .build();
        let cancel_btn = Button::builder().label("Cancel").build();
        let s = sender.clone();
        cancel_btn.connect_clicked(move |_| {
            let _ = s.output(PkceLoginDialogOutput::Cancelled);
        });
        action_row.append(&cancel_btn);

        let submit_btn = Button::builder()
            .label("Sign In")
            .css_classes(["suggested-action"])
            .build();
        let s = sender.clone();
        let redirect_entry_for_submit = redirect_entry.clone();
        submit_btn.connect_clicked(move |_| {
            let url = redirect_entry_for_submit.text().to_string();
            if !url.trim().is_empty() {
                let _ = s.output(PkceLoginDialogOutput::Submit(url));
            }
        });
        // Enter inside the entry triggers Submit.
        let s = sender.clone();
        redirect_entry.connect_activate(move |e| {
            let url = e.text().to_string();
            if !url.trim().is_empty() {
                let _ = s.output(PkceLoginDialogOutput::Submit(url));
            }
        });
        action_row.append(&submit_btn);
        body.append(&action_row);

        let status_label = Label::builder()
            .label("Waiting…")
            .xalign(0.0)
            .css_classes(["dim-label"])
            .build();
        body.append(&status_label);

        let s = sender.clone();
        root.connect_close_request(move |_| {
            let _ = s.output(PkceLoginDialogOutput::Cancelled);
            glib::Propagation::Proceed
        });

        root.set_content(Some(&body));

        let model = Self {
            auth_url: None,
            status: "Waiting for authorize URL…".into(),
            busy: false,
        };
        let widgets = PkceLoginDialogWidgets {
            window: root,
            url_label,
            redirect_entry,
            status_label,
            submit_btn,
        };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            PkceLoginDialogInput::Show(url) => {
                self.auth_url = Some(url);
                self.status = "Open the URL, sign in, then paste the redirect URL.".into();
                self.busy = false;
            }
            PkceLoginDialogInput::SetStatus(s) => {
                self.status = s;
            }
            PkceLoginDialogInput::SetBusy(b) => {
                self.busy = b;
                if b {
                    self.status = "Validating…".into();
                }
            }
            PkceLoginDialogInput::Hide => {
                self.auth_url = None;
                self.busy = false;
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        match &self.auth_url {
            Some(url) => {
                widgets.url_label.set_label(url);
                widgets.status_label.set_label(&self.status);
                widgets.submit_btn.set_sensitive(!self.busy);
                widgets.redirect_entry.set_sensitive(!self.busy);
                if !widgets.window.is_visible() {
                    widgets.window.present();
                }
            }
            None => {
                if widgets.window.is_visible() {
                    widgets.window.set_visible(false);
                }
                widgets.redirect_entry.set_text("");
            }
        }
    }
}
