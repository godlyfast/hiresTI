//! Crate-wide error type. Domain modules wrap their own errors but everything
//! that bubbles up to the UI layer collapses into `AppError` so view code only
//! has to surface a single variant.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("config dir not resolvable: {0}")]
    ConfigDir(String),

    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("settings parse failed: {0}")]
    Settings(#[from] serde_json::Error),
}

pub type AppResult<T> = std::result::Result<T, AppError>;

impl AppError {
    pub fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
