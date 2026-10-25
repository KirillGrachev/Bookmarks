//! Runs core operations off the window event loop.
//!
//! Every command that touches SQLite, the settings file or the vault goes through
//! [`blocking`]. The reason is concrete: a synchronous command runs on the async
//! runtime's worker, and a long write transaction holds the store mutex — the
//! window then stops responding to input, dragging and the cursor until it is
//! released. Executing on the blocking pool keeps the UI thread free.

use bookmarks_core::db::Store;

use crate::error::{CommandError, CommandResult};

/// Executes a store operation on the blocking pool and awaits the result.
pub async fn blocking<T, F>(store: Store, operation: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce(&Store) -> bookmarks_core::Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || operation(&store).map_err(CommandError::from))
        .await
        .map_err(|error| join_error(error, "store task failed"))?
}

/// Executes an arbitrary closure on the blocking pool (file IO, sync, export).
pub async fn blocking_task<T, F>(task: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> CommandResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|error| join_error(error, "background task failed"))?
}

/// Converts a join error into the IPC error shape.
///
/// A join error means the task panicked. Reporting it as a constraint fault keeps
/// the frontend contract (`kind`/`message`/`field`) intact instead of leaking a
/// runtime error type the UI cannot classify. The concrete error type is left
/// generic because `tauri::async_runtime` does not re-export it.
fn join_error<E: std::fmt::Display>(error: E, context: &str) -> CommandError {
    CommandError::new(bookmarks_core::Error::Constraint(format!("{context}: {error}")).into())
}
