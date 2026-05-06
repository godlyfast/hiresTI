//! `hiresti` GTK4 / libadwaita / Relm4 entry point.
//!
//! This binary is the eventual replacement for `python3 src/main.py`. It
//! boots the GTK application, loads persisted settings, and hands off to
//! the Relm4 component graph.
//!
//! See `docs/refactor-plan.md` (TODO) for the full migration plan; the
//! short version is: Phase 1 = empty shell + settings round-trip, then
//! views land top-down in subsequent phases.

mod app;
mod components;
mod error;
mod messages;
mod model;
mod paths;
mod services;
mod settings;
mod state;

use std::process::ExitCode;

use tracing_subscriber::EnvFilter;

use crate::error::AppResult;
use crate::settings::Settings;

fn main() -> ExitCode {
    init_logging();
    if let Err(e) = paths::ensure_dirs() {
        tracing::error!(error = %e, "failed to create config/cache dirs");
        return ExitCode::from(1);
    }

    let settings = match Settings::load() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "failed to load settings; using defaults");
            Settings::default()
        }
    };
    tracing::info!(
        window_size = ?(settings.window_width, settings.window_height),
        last_nav = %settings.last_nav,
        "settings loaded"
    );

    let code = app::run(settings);
    ExitCode::from(code as u8)
}

fn init_logging() {
    // `HIRESTI_LOG=trace` etc. for verbose, default to info. Format
    // is intentionally close to the existing Python logger output so
    // long-running diagnostics workflows stay readable.
    let filter = EnvFilter::try_from_env("HIRESTI_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .try_init();
}

// Suppress the "unused" lint on AppResult while the type has only one user;
// later phases will surface it through `main()` as the error-flow trunk.
#[allow(dead_code)]
fn _result_marker() -> AppResult<()> {
    Ok(())
}
