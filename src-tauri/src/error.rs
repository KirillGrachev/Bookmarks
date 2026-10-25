//! IPC error type.
//!
//! Every command returns `CommandResult<T>`, whose error serializes into the
//! `ErrorPayload` shape the frontend expects (`{ kind, message, field?, code? }`).
//! Tauri requires command errors to be serializable, and a plain `String` would
//! throw away the field and code the UI needs to highlight a form input or to
//! ask for the vault password again.

use bookmarks_core::error::ErrorPayload;
use serde::Serialize;

/// Error returned to the frontend.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct CommandError {
    payload: ErrorPayload,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Logs and startup errors need a readable message; the frontend uses the
        // serialized payload instead.
        write!(formatter, "{}", self.payload.message)
    }
}

impl std::error::Error for CommandError {}

impl CommandError {
    /// Wraps a core error.
    pub fn new(payload: ErrorPayload) -> Self {
        Self { payload }
    }
}

impl From<bookmarks_core::Error> for CommandError {
    fn from(error: bookmarks_core::Error) -> Self {
        Self::new(error.into())
    }
}

impl From<serde_json::Error> for CommandError {
    fn from(error: serde_json::Error) -> Self {
        Self::new(bookmarks_core::Error::from(error).into())
    }
}

impl From<std::io::Error> for CommandError {
    fn from(error: std::io::Error) -> Self {
        Self::new(bookmarks_core::Error::from(error).into())
    }
}

/// Result alias used by every command.
pub type CommandResult<T> = Result<T, CommandError>;
