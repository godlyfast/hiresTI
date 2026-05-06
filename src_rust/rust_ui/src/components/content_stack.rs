//! Content stack: 13 stack pages, one per nav target. Phase 1 shipped
//! placeholder labels; Phase 5 lets the parent supply real widgets for
//! the library nav pages (Albums / Tracks / Artists / Playlists /
//! Mixes / History). Targets without a supplied widget keep the
//! placeholder.

use relm4::gtk::{self, Label, Stack, StackTransitionType, Widget};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use crate::messages::NavTarget;

const ALL_TARGETS: [NavTarget; 13] = [
    NavTarget::Home,
    NavTarget::New,
    NavTarget::Top,
    NavTarget::HiRes,
    NavTarget::Genres,
    NavTarget::Decades,
    NavTarget::Moods,
    NavTarget::Albums,
    NavTarget::Tracks,
    NavTarget::Artists,
    NavTarget::Playlists,
    NavTarget::MixesAndRadio,
    NavTarget::History,
];

pub struct ContentStackModel {
    current: NavTarget,
}

#[derive(Debug, Clone, Copy)]
pub enum ContentStackInput {
    Show(NavTarget),
}

pub struct ContentStackInit {
    pub current: NavTarget,
    /// Widgets the parent has already constructed for specific nav
    /// targets. Targets without an entry get a placeholder label.
    pub pages: Vec<(NavTarget, Widget)>,
}

pub struct ContentStackWidgets {
    stack: Stack,
}

impl SimpleComponent for ContentStackModel {
    type Init = ContentStackInit;
    type Input = ContentStackInput;
    type Output = ();
    type Root = Stack;
    type Widgets = ContentStackWidgets;

    fn init_root() -> Self::Root {
        Stack::builder()
            .transition_type(StackTransitionType::Crossfade)
            .transition_duration(180)
            .vexpand(true)
            .hexpand(true)
            .build()
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        for target in ALL_TARGETS {
            // Caller-supplied widget wins.
            let supplied = init
                .pages
                .iter()
                .find(|(t, _)| *t == target)
                .map(|(_, w)| w.clone());
            match supplied {
                Some(w) => {
                    root.add_named(&w, Some(target.as_id()));
                }
                None => {
                    let placeholder = Label::builder()
                        .label(format!(
                            "{}\n(view body lands in Phase 6+)",
                            target.label()
                        ))
                        .justify(gtk::Justification::Center)
                        .vexpand(true)
                        .hexpand(true)
                        .css_classes(["dim-label"])
                        .build();
                    root.add_named(&placeholder, Some(target.as_id()));
                }
            }
        }
        root.set_visible_child_name(init.current.as_id());

        let widgets = ContentStackWidgets {
            stack: root.clone(),
        };
        ComponentParts {
            model: Self {
                current: init.current,
            },
            widgets,
        }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            ContentStackInput::Show(target) => {
                self.current = target;
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        widgets.stack.set_visible_child_name(self.current.as_id());
    }
}
