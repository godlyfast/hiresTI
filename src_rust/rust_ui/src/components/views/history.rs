//! Recent / History view. Reads from the local
//! `~/.cache/hiresti/profiles/<scope>/history.json` instead of the
//! TIDAL API. Phase 5 ships an empty-state stub; the persistent
//! history store wiring lands when Phase 7's playback flow starts
//! recording entries.

use relm4::gtk::{self, ScrolledWindow};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use crate::components::views::common::{build_empty_widget, LibraryViewOutput};

pub struct HistoryViewModel;

#[derive(Debug, Clone)]
pub enum HistoryViewInput {
    Refresh,
}

impl SimpleComponent for HistoryViewModel {
    type Init = ();
    type Input = HistoryViewInput;
    type Output = LibraryViewOutput;
    type Root = ScrolledWindow;
    type Widgets = ();

    fn init_root() -> Self::Root {
        ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .hexpand(true)
            .build()
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        root.set_child(Some(&build_empty_widget(
            "Recently played tracks will appear here once Phase 7 wires up playback recording.",
        )));
        ComponentParts {
            model: Self,
            widgets: (),
        }
    }

    fn update(&mut self, _msg: Self::Input, _sender: ComponentSender<Self>) {
        // Refresh becomes meaningful once the history store is wired.
    }
}
