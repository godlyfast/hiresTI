//! Sidebar navigation. Renders the 13 nav targets grouped into the
//! three sections defined in `messages::NavSection`. Click on a row →
//! emit `SidebarOutput::Navigate(target)` which the root forwards into
//! `AppInput::NavigateTo`.

use relm4::gtk::{
    self, prelude::*, Box as GtkBox, Image, Label, ListBox, ListBoxRow, Orientation,
};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use crate::messages::{NavSection, NavTarget};

pub struct SidebarModel {
    current: NavTarget,
}

#[derive(Debug, Clone, Copy)]
pub enum SidebarInput {
    /// Root tells us to highlight a row (e.g. on cold-start when
    /// `settings.last_nav` selects a non-default target).
    SetActive(NavTarget),
}

#[derive(Debug, Clone, Copy)]
pub enum SidebarOutput {
    Navigate(NavTarget),
}

pub struct SidebarWidgets {
    /// One ListBox per section so each gets its own selection state;
    /// we clear the others when one fires its row-activated signal so
    /// only one row across the sidebar appears highlighted.
    sections: Vec<(NavSection, ListBox, Vec<(NavTarget, ListBoxRow)>)>,
}

impl SimpleComponent for SidebarModel {
    type Init = NavTarget;
    type Input = SidebarInput;
    type Output = SidebarOutput;
    type Root = GtkBox;
    type Widgets = SidebarWidgets;

    fn init_root() -> Self::Root {
        GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(4)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(4)
            .margin_end(4)
            .width_request(200)
            .css_classes(["sidebar-root"])
            .build()
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let mut sections = Vec::new();
        for section in NavSection::ALL.iter().copied() {
            // Section header ("Discover" / "Your Library" / "Recent")
            let header = Label::builder()
                .label(section.label())
                .xalign(0.0)
                .margin_top(8)
                .margin_bottom(2)
                .margin_start(8)
                .css_classes(["dim-label", "sidebar-section-header"])
                .build();
            root.append(&header);

            let list = ListBox::builder()
                .selection_mode(gtk::SelectionMode::Single)
                .css_classes(["navigation-sidebar"])
                .build();

            let mut rows = Vec::new();
            for target in section.targets().iter().copied() {
                let row = build_row(target);
                list.append(&row);
                rows.push((target, row));
            }

            // Per-list row activation: emit Navigate, plus clear other
            // sections so visually only one row is selected.
            let sender_clone = sender.clone();
            let section_id = section;
            list.connect_row_activated(move |_lb, row| {
                if let Some(t) = row_target(row) {
                    let _ = sender_clone.output(SidebarOutput::Navigate(t));
                }
                let _ = section_id; // suppress capture-only warning
            });

            root.append(&list);
            sections.push((section, list, rows));
        }

        let model = Self { current: init };
        let widgets = SidebarWidgets { sections };

        // Apply initial highlight. Done after construction so all rows
        // exist; running select_row on a row before it's parented is
        // a no-op.
        select_row_for(&widgets, init);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            SidebarInput::SetActive(target) => {
                self.current = target;
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        select_row_for(widgets, self.current);
    }
}

/// Symbolic Adwaita icon for each nav target. Symbolic variants
/// recolor with the theme so they look right in both light + dark.
fn icon_for(target: NavTarget) -> &'static str {
    match target {
        NavTarget::Home => "go-home-symbolic",
        NavTarget::New => "starred-symbolic",
        NavTarget::Top => "view-pin-symbolic",
        NavTarget::HiRes => "audio-headphones-symbolic",
        NavTarget::Genres => "applications-multimedia-symbolic",
        NavTarget::Decades => "office-calendar-symbolic",
        NavTarget::Moods => "face-smile-symbolic",
        NavTarget::Albums => "media-optical-symbolic",
        NavTarget::Tracks => "audio-x-generic-symbolic",
        NavTarget::Artists => "system-users-symbolic",
        NavTarget::Playlists => "view-list-symbolic",
        NavTarget::MixesAndRadio => "media-playlist-shuffle-symbolic",
        NavTarget::History => "document-open-recent-symbolic",
    }
}

/// Build a single sidebar row. The row carries its `NavTarget` as a
/// glib data key so the row-activated handler can recover it without
/// per-row closures (which would require boxing the sender per row).
fn build_row(target: NavTarget) -> ListBoxRow {
    let body = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(10)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(12)
        .margin_end(8)
        .build();
    let icon = Image::builder()
        .icon_name(icon_for(target))
        .pixel_size(16)
        .build();
    body.append(&icon);
    let label = Label::builder()
        .label(target.label())
        .xalign(0.0)
        .hexpand(true)
        .build();
    body.append(&label);
    let row = ListBoxRow::builder()
        .child(&body)
        .css_classes(["sidebar-row"])
        .build();
    // Tag the row with its target id so row_target() can read it back.
    unsafe {
        row.set_data(ROW_TARGET_KEY, target.as_id());
    }
    row
}

const ROW_TARGET_KEY: &str = "hiresti-nav-target";

fn row_target(row: &ListBoxRow) -> Option<NavTarget> {
    let id: &&'static str = unsafe { row.data(ROW_TARGET_KEY)?.as_ref() };
    NavTarget::from_id(id)
}

fn select_row_for(widgets: &SidebarWidgets, target: NavTarget) {
    for (_section, list, rows) in widgets.sections.iter() {
        let mut hit = false;
        for (t, row) in rows.iter() {
            if *t == target {
                list.select_row(Some(row));
                hit = true;
            }
        }
        if !hit {
            list.unselect_all();
        }
    }
}
