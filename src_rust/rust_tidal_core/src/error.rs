use serde::Serialize;

/// Error kind aligned with src/core/errors.py classify_exception() so the
/// existing Python session-recovery logic can keep dispatching on the same
/// strings: "auth" | "network" | "server" | "not_found" | "client" | "unknown".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Auth,
    Network,
    Server,
    NotFound,
    Client,
    Unknown,
}

#[derive(Debug, thiserror::Error)]
pub enum RtcError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("auth: {0}")]
    Auth(String),
    #[error("network: {0}")]
    Network(String),
    #[error("server error {status}: {body}")]
    Server { status: u16, body: String },
    #[error("not found: {0}")]
    NotFound(String),
    #[error("client error {status}: {body}")]
    Client { status: u16, body: String },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("url: {0}")]
    Url(#[from] url::ParseError),
    #[error("{0}")]
    Other(String),
}

impl RtcError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            RtcError::Auth(_) => ErrorKind::Auth,
            RtcError::Network(_) => ErrorKind::Network,
            RtcError::Server { .. } => ErrorKind::Server,
            RtcError::NotFound(_) => ErrorKind::NotFound,
            RtcError::Client { .. } => ErrorKind::Client,
            RtcError::InvalidInput(_) => ErrorKind::Client,
            RtcError::Io(_) | RtcError::Json(_) | RtcError::Url(_) | RtcError::Other(_) => {
                ErrorKind::Unknown
            }
        }
    }
}

#[derive(Serialize)]
pub struct ErrorPayload<'a> {
    pub error: String,
    pub kind: ErrorKind,
    pub status: Option<u16>,
    pub message: &'a str,
}

impl<'a> ErrorPayload<'a> {
    pub fn from(err: &'a RtcError) -> Self {
        let (kind, status, message) = match err {
            RtcError::Server { status, body } => (ErrorKind::Server, Some(*status), body.as_str()),
            RtcError::Client { status, body } => (ErrorKind::Client, Some(*status), body.as_str()),
            _ => (err.kind(), None, ""),
        };
        let message = if message.is_empty() {
            "see error field for human-readable description"
        } else {
            message
        };
        Self {
            error: err.to_string(),
            kind,
            status,
            message,
        }
    }
}

pub type RtcResult<T> = Result<T, RtcError>;

/// Map an ureq transport error into RtcError.
pub fn map_ureq(err: ureq::Error) -> RtcError {
    match err {
        ureq::Error::Status(status, response) => {
            let body = response
                .into_string()
                .unwrap_or_else(|_| "<unreadable body>".to_string());
            match status {
                401 | 403 => RtcError::Auth(format!("HTTP {}: {}", status, body)),
                404 => RtcError::NotFound(format!("HTTP 404: {}", body)),
                500..=599 => RtcError::Server { status, body },
                _ => RtcError::Client { status, body },
            }
        }
        ureq::Error::Transport(t) => RtcError::Network(t.to_string()),
    }
}
