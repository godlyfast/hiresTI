//! Header bar: title widget + search entry + login button + settings menu.
//!
//! Phase 3 wires only the structure — search routes a typed-in query
//! through `HeaderOutput::Search` but the search subsystem isn't built
//! yet (Phase 7), and the login/settings buttons fire output events
//! the root currently no-ops on. The wiring boundary is in place so
//! Phase 4+ can consume these outputs without touching this file.

use libadwaita::prelude::*;
use libadwaita::HeaderBar;
use relm4::gtk::{self, Box as GtkBox, Button, Entry, Label, MenuButton};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

pub struct HeaderModel {
    logged_in_label: String,
    /// True while a detail page is open. Drives the back-button visibility
    /// — the root flips this via `SetDetailOpen` whenever it installs or
    /// closes a detail surface.
    detail_open: bool,
}

#[derive(Debug, Clone)]
pub enum HeaderInput {
    SetUserDisplay(Option<String>),
    SetDetailOpen(bool),
}

#[derive(Debug, Clone)]
pub enum HeaderOutput {
    Search(String),
    LoginRequested,
    OpenSettings,
    OpenAbout,
    OpenDiagnostics,
    OpenSignalPath,
    OpenDspPresets,
    BackPressed,
}

pub struct HeaderWidgets {
    bar: HeaderBar,
    user_label: Label,
    back_btn: Button,
}

impl SimpleComponent for HeaderModel {
    type Init = ();
    type Input = HeaderInput;
    type Output = HeaderOutput;
    type Root = HeaderBar;
    type Widgets = HeaderWidgets;

    fn init_root() -> Self::Root {
        HeaderBar::builder().build()
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // Back button: hidden until a detail surface opens.
        let back_btn = Button::builder()
            .icon_name("go-previous-symbolic")
            .tooltip_text("Back")
            .visible(false)
            .build();
        let s = sender.clone();
        back_btn.connect_clicked(move |_| {
            let _ = s.output(HeaderOutput::BackPressed);
        });
        root.pack_start(&back_btn);

        let title = GtkBox::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        let app_label = Label::builder()
            .label("HiresTI")
            .css_classes(["heading"])
            .build();
        title.append(&app_label);
        root.set_title_widget(Some(&title));

        // Center: search entry. PhASe 3 just emits typed-in queries;
        // dispatch / completion / history popover lands later.
        let search = Entry::builder()
            .placeholder_text("Search…")
            .width_request(360)
            .build();
        let s = sender.clone();
        search.connect_activate(move |entry| {
            let q = entry.text().to_string();
            if !q.trim().is_empty() {
                let _ = s.output(HeaderOutput::Search(q));
            }
        });
        root.pack_start(&search);

        // Right: settings menu, then login button at the very end.
        let settings_btn = MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Settings")
            .build();
        // Build a popover with About / Settings entries. Both emit to
        // the parent; popover state itself isn't tracked.
        let popover = build_settings_popover(sender.clone());
        settings_btn.set_popover(Some(&popover));
        root.pack_end(&settings_btn);

        let user_label = Label::builder()
            .label("Login")
            .css_classes(["caption", "dim-label"])
            .build();
        let login_btn = Button::builder().child(&user_label).build();
        let s = sender.clone();
        login_btn.connect_clicked(move |_| {
            let _ = s.output(HeaderOutput::LoginRequested);
        });
        root.pack_end(&login_btn);

        let model = Self {
            logged_in_label: "Login".into(),
            detail_open: false,
        };
        let widgets = HeaderWidgets {
            bar: root,
            user_label,
            back_btn,
        };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            HeaderInput::SetUserDisplay(name) => {
                self.logged_in_label = name
                    .filter(|n| !n.trim().is_empty())
                    .map(|n| format!("Hi, {n}"))
                    .unwrap_or_else(|| "Login".into());
            }
            HeaderInput::SetDetailOpen(open) => {
                self.detail_open = open;
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        widgets.user_label.set_label(&self.logged_in_label);
        widgets.back_btn.set_visible(self.detail_open);
        let _ = &widgets.bar;
    }
}

fn build_settings_popover(sender: ComponentSender<HeaderModel>) -> gtk::Popover {
    let pop = gtk::Popover::new();
    let lb = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();

    let settings_row = gtk::ListBoxRow::builder()
        .child(
            &Label::builder()
                .label("Settings")
                .xalign(0.0)
                .margin_top(6)
                .margin_bottom(6)
                .margin_start(12)
                .margin_end(12)
                .build(),
        )
        .build();
    lb.append(&settings_row);

    let about_row = gtk::ListBoxRow::builder()
        .child(
            &Label::builder()
                .label("About")
                .xalign(0.0)
                .margin_top(6)
                .margin_bottom(6)
                .margin_start(12)
                .margin_end(12)
                .build(),
        )
        .build();
    lb.append(&about_row);

    let diag_row = gtk::ListBoxRow::builder()
        .child(
            &Label::builder()
                .label("Diagnostics")
                .xalign(0.0)
                .margin_top(6)
                .margin_bottom(6)
                .margin_start(12)
                .margin_end(12)
                .build(),
        )
        .build();
    lb.append(&diag_row);

    let path_row = gtk::ListBoxRow::builder()
        .child(
            &Label::builder()
                .label("Signal Path")
                .xalign(0.0)
                .margin_top(6)
                .margin_bottom(6)
                .margin_start(12)
                .margin_end(12)
                .build(),
        )
        .build();
    lb.append(&path_row);

    let presets_row = gtk::ListBoxRow::builder()
        .child(
            &Label::builder()
                .label("DSP Presets")
                .xalign(0.0)
                .margin_top(6)
                .margin_bottom(6)
                .margin_start(12)
                .margin_end(12)
                .build(),
        )
        .build();
    lb.append(&presets_row);

    let pop_clone = pop.clone();
    let sender_clone = sender.clone();
    lb.connect_row_activated(move |_, row| {
        pop_clone.popdown();
        if row == &settings_row {
            let _ = sender_clone.output(HeaderOutput::OpenSettings);
        } else if row == &about_row {
            let _ = sender_clone.output(HeaderOutput::OpenAbout);
        } else if row == &diag_row {
            let _ = sender_clone.output(HeaderOutput::OpenDiagnostics);
        } else if row == &path_row {
            let _ = sender_clone.output(HeaderOutput::OpenSignalPath);
        } else if row == &presets_row {
            let _ = sender_clone.output(HeaderOutput::OpenDspPresets);
        }
    });

    pop.set_child(Some(&lb));
    pop
}
