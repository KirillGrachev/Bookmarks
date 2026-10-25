//! Sync commands.
//!
//! The engine itself lives in the core; this module only builds the transport
//! from settings, runs the engine on the blocking pool and reports progress to
//! the window as events.
//!
//! Note on state access: `tauri::State` borrows from the invoke context and
//! cannot cross into a `'static` task, so everything a task needs is cloned out
//! first. `Store` is a cheap handle over a shared connection pool, and settings
//! are read as a snapshot — a sync run must not see settings change under it.

use std::sync::Arc;

use bookmarks_core::domain::{RemoteKind, SyncSettings};
use bookmarks_core::sync::{
    ConflictPolicy, DirectoryRemote, KnownDevice, Remote, SyncEngine, SyncOutcome, SyncPlan,
};
use bookmarks_core::Error;
use serde::Serialize;

use crate::blocking::blocking_task;
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

/// Event payload emitted while syncing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    /// Phase name (`started`, `planning`, `finished`, `error`).
    pub state: String,
    /// Human-readable detail.
    pub detail: Option<String>,
}

/// Everything a sync task needs, cloned out of the managed state.
struct SyncContext {
    store: bookmarks_core::db::Store,
    sync: Arc<crate::state::SyncState>,
    settings: SyncSettings,
    device_id: String,
    device_name: String,
}

impl SyncContext {
    fn from_state(state: &AppState) -> Self {
        Self {
            store: state.store.clone(),
            sync: Arc::clone(&state.sync),
            settings: state.settings().sync,
            device_id: state.device_id.clone(),
            device_name: state.device_name(),
        }
    }

    /// Builds the engine described by the settings snapshot.
    fn engine(&self, policy: ConflictPolicy) -> CommandResult<SyncEngine> {
        let remote: Arc<dyn Remote> =
            match self.settings.remote {
                RemoteKind::None => {
                    return Err(CommandError::new(
                        Error::invalid("sync is disabled in settings").into(),
                    ));
                }
                RemoteKind::Directory => {
                    if self.settings.location.trim().is_empty() {
                        return Err(CommandError::new(
                            Error::invalid("sync folder is not set").into(),
                        ));
                    }
                    Arc::new(DirectoryRemote::new(self.settings.location.trim()).map_err(
                        |error| CommandError::new(Error::invalid(error.to_string()).into()),
                    )?)
                }
                RemoteKind::WebDav => {
                    return Err(CommandError::new(
                        Error::sync(
                            "WebDAV transport is not implemented yet; mount it as a folder instead",
                            bookmarks_core::error::SyncErrorCode::RemoteFormat,
                        )
                        .into(),
                    ));
                }
            };

        let engine = SyncEngine::new(
            self.store.clone(),
            remote,
            self.device_id.clone(),
            self.device_name.clone(),
            policy,
        );

        if !self.settings.encrypt_remote {
            return Ok(engine);
        }
        // E2EE must not silently degrade to plaintext because the vault happens
        // to be locked: refuse the run and tell the user to unlock it.
        match self.store.vault_current_key().map_err(CommandError::from)? {
            Some(key) => Ok(engine.with_encryption(key)),
            None => Err(CommandError::new(
                Error::vault(
                    "encrypted sync needs an unlocked vault: unlock it or turn off remote encryption",
                    bookmarks_core::error::VaultErrorCode::BadPassword,
                )
                .into(),
            )),
        }
    }
}

/// Current sync state for the header chip.
#[tauri::command]
pub fn sync_status(state: tauri::State<'_, AppState>) -> CommandResult<crate::state::SyncStatus> {
    Ok(state.sync.snapshot())
}

/// Builds the sync plan without applying it.
#[tauri::command]
pub async fn sync_plan(
    policy: ConflictPolicy,
    state: tauri::State<'_, AppState>,
) -> CommandResult<SyncPlan> {
    let context = SyncContext::from_state(&state);
    blocking_task(move || {
        let engine = context.engine(policy)?;
        engine.plan().map_err(CommandError::from)
    })
    .await
}

/// Runs a full sync.
///
/// Concurrent runs are rejected rather than queued: two engines publishing a
/// manifest for the same device would race, and the UI already disables the
/// button while `running` is true.
#[tauri::command]
pub async fn sync_run(
    app: tauri::AppHandle,
    policy: ConflictPolicy,
    state: tauri::State<'_, AppState>,
) -> CommandResult<SyncOutcome> {
    let context = SyncContext::from_state(&state);
    let tracker = Arc::clone(&context.sync);
    if !tracker.begin() {
        return Err(CommandError::new(
            Error::sync(
                "sync is already running",
                bookmarks_core::error::SyncErrorCode::Local,
            )
            .into(),
        ));
    }

    let result = blocking_task(move || {
        emit(&app, "started", None);
        let engine = match context.engine(policy) {
            Ok(engine) => engine,
            Err(error) => {
                emit(&app, "error", Some("cannot prepare sync"));
                tracker.finish(Some("cannot prepare sync".to_owned()));
                return Err(error);
            }
        };
        emit(&app, "planning", Some(engine.remote_description().as_str()));
        match engine.run() {
            Ok(outcome) => {
                let detail = format!(
                    "uploaded {}, downloaded {}, conflicts {}",
                    outcome.uploaded, outcome.downloaded, outcome.conflicts_resolved
                );
                emit(&app, "finished", Some(detail.as_str()));
                tracker.finish(None);
                Ok(outcome)
            }
            Err(error) => {
                let message = error.to_string();
                emit(&app, "error", Some(message.as_str()));
                tracker.finish(Some(message));
                Err(CommandError::from(error))
            }
        }
    })
    .await;

    // If the task itself failed to run (panic before `finish`), release the flag;
    // otherwise sync would stay "running" until the app restarts.
    if result.is_err() && state.sync.is_running() {
        state.sync.finish(Some("sync task failed".to_owned()));
    }
    result
}

/// Checks that the storage is reachable and writable.
#[tauri::command]
pub async fn sync_probe(state: tauri::State<'_, AppState>) -> CommandResult<()> {
    let context = SyncContext::from_state(&state);
    blocking_task(move || {
        let engine = context.engine(ConflictPolicy::KeepNewest)?;
        engine.probe().map_err(CommandError::from)
    })
    .await
}

/// Lists other devices that published manifests.
#[tauri::command]
pub async fn sync_devices(state: tauri::State<'_, AppState>) -> CommandResult<Vec<KnownDevice>> {
    let context = SyncContext::from_state(&state);
    blocking_task(move || {
        let engine = context.engine(ConflictPolicy::KeepNewest)?;
        engine.known_devices().map_err(CommandError::from)
    })
    .await
}

fn emit(app: &tauri::AppHandle, state: &str, detail: Option<&str>) {
    use tauri::Emitter;
    let payload = SyncProgress {
        state: state.to_owned(),
        detail: detail.map(str::to_owned),
    };
    if let Err(error) = app.emit("sync-progress", payload) {
        tracing::warn!(%error, "failed to emit sync progress");
    }
}
