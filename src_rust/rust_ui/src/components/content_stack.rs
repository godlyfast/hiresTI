//! Content stack: 13 stack pages, one per nav target. Phase 3 fills
//! each with a placeholder label so navigation is observable. Phase 5+
//! will replace each placeholder with the real view component.

use relm4::gtk::{self, Label, Stack, StackTransitionType};
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

pub struct ContentStackWidgets {
    /// Cloned reference to the root Stack so update_view can flip
    /// `visible_child_name` without going through the component root.
    /// (Cheap clone: GTK widgets are reference-counted handles.)
    stack: Stack,
}

impl SimpleComponent for ContentStackModel {
    type Init = NavTarget;
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
            let placeholder = Label::builder()
                .label(format!(
                    "{}\n(view body lands in Phase 5+)",
                    target.label()
                ))
                .justify(gtk::Justification::Center)
                .vexpand(true)
                .hexpand(true)
                .css_classes(["dim-label"])
                .build();
            root.add_named(&placeholder, Some(target.as_id()));
        }
        root.set_visible_child_name(init.as_id());

        let widgets = ContentStackWidgets {
            stack: root.clone(),
        };
        ComponentParts {
            model: Self { current: init },
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
