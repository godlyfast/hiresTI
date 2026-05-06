//! Root application state. Phase 1 keeps this minimal — just the bits
//! needed to boot the empty shell and persist window geometry / nav
//! position. Subsequent phases extend with `LibraryState`, `PlaybackState`,
//! etc., as separate sub-models nested into `AppModel`.

use crate::messages::NavTarget;
use crate::settings::Settings;

#[derive(Debug, Clone)]
pub struct AppModel {
    pub settings: Settings,
    pub current_nav: NavTarget,
}

impl AppModel {
    pub fn from_settings(settings: Settings) -> Self {
        let current_nav =
            NavTarget::from_id(&settings.last_nav).unwrap_or(NavTarget::Home);
        Self {
            settings,
            current_nav,
        }
    }
}
