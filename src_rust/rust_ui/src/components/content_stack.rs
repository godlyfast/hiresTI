//! Content stack: 13 stack pages, one per nav target, plus a single
//! "__detail" slot the root component swaps into when an album / playlist
//! / artist page is open. Phase 1 shipped placeholder labels; Phase 5 lets
//! the parent supply real widgets for the library nav pages; Phase 7-A
//! adds the detail slot.

use relm4::gtk::{self, Label, Stack, StackTransitionType, Widget};
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use crate::messages::NavTarget;

const DETAIL_SLOT: &str = "__detail";

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
    /// True while a detail page is the visible child. update_view uses
    /// this to decide whether to flip back to the nav target after a
    /// SetDetail(None).
    detail_active: bool,
    /// Owned Stack clone so update() (which doesn't get Widgets) can
    /// add/remove the detail child directly.
    stack: Stack,
    /// The widget currently installed as "__detail" — kept around so we
    /// can `stack.remove(&old)` before adding a replacement.
    detail_widget: Option<Widget>,
}

#[derive(Debug, Clone)]
pub enum ContentStackInput {
    Show(NavTarget),
    /// Install `widget` as the detail slot and switch to showing it.
    /// `None` clears the detail and returns to the active nav target.
    SetDetail(Option<Widget>),
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
                detail_active: false,
                stack: root.clone(),
                detail_widget: None,
            },
            widgets,
        }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            ContentStackInput::Show(target) => {
                self.current = target;
                // Showing a top-level nav target also clears any open
                // detail — sidebar clicks should drop you back into the
                // library/discovery page, not stay buried in detail.
                self.clear_detail();
                self.detail_active = false;
            }
            ContentStackInput::SetDetail(maybe_widget) => {
                self.clear_detail();
                match maybe_widget {
                    Some(w) => {
                        self.stack.add_named(&w, Some(DETAIL_SLOT));
                        self.detail_widget = Some(w);
                        self.detail_active = true;
                    }
                    None => {
                        self.detail_active = false;
                    }
                }
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        let visible = if self.detail_active {
            DETAIL_SLOT
        } else {
            self.current.as_id()
        };
        widgets.stack.set_visible_child_name(visible);
    }
}

impl ContentStackModel {
    fn clear_detail(&mut self) {
        if let Some(old) = self.detail_widget.take() {
            self.stack.remove(&old);
        }
    }
}
