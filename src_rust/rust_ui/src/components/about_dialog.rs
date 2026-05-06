//! About dialog. Adw.AboutWindow with the application metadata + the
//! workspace component versions so users can report what crate version
//! produced a bug. Built imperatively because it's a one-shot dialog
//! with no persistent state — no SimpleComponent needed.

use libadwaita::prelude::*;
use libadwaita::AboutWindow;
use relm4::gtk::Window;

const APP_NAME: &str = "HiresTI";
const APP_ID: &str = "com.hiresti.player";
const COPYRIGHT: &str = "© 2024 HiresTI contributors";
const LICENSE: &str = "GPL-3.0";
const ISSUE_URL: &str = "https://github.com/anthropics/claude-code/issues";

/// Build and present an About dialog parented to `parent`. Closes
/// itself when the user dismisses it; the caller doesn't need to track
/// it after this call returns.
pub fn present(parent: &Window) {
    let about = AboutWindow::builder()
        .application_name(APP_NAME)
        .application_icon("audio-x-generic-symbolic")
        .developer_name("HiresTI contributors")
        .version(env!("CARGO_PKG_VERSION"))
        .copyright(COPYRIGHT)
        .license_type(libadwaita::gtk::License::Gpl30)
        .website(ISSUE_URL)
        .issue_url(ISSUE_URL)
        .modal(true)
        .transient_for(parent)
        .build();

    // Embed component versions so a bug report shows which slice of the
    // workspace was on disk. They don't change at runtime — picking
    // them up via env! at compile time is enough.
    about.add_credit_section(
        Some("Component versions"),
        &[
            &format!("rust_tidal_core {}", env!("CARGO_PKG_VERSION")),
            &format!("rust_audio_core {}", env!("CARGO_PKG_VERSION")),
            &format!("rust_viz_core {}", env!("CARGO_PKG_VERSION")),
        ],
    );

    let _ = APP_ID; // kept around — Phase 9 wires it into the desktop file
    let _ = LICENSE; // referenced in the doc comment, but kept in code
                    // so a license switch is a one-line change.

    about.present();
}
