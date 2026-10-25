//! Version history commands.

use bookmarks_core::domain::note::Note;
use bookmarks_core::ids::EntityId;
use bookmarks_core::versions::{NoteVersion, VersionSummary};

use crate::blocking::blocking;
use crate::error::CommandResult;
use crate::state::AppState;

/// Lists versions of a note, newest first, without bodies.
#[tauri::command]
pub async fn list_versions(
    note_id: EntityId,
    limit: u32,
    state: tauri::State<'_, AppState>,
) -> CommandResult<Vec<VersionSummary>> {
    let store = state.store.clone();
    blocking(store, move |store| store.list_versions(&note_id, limit)).await
}

/// Returns one version with its body.
#[tauri::command]
pub async fn get_version(
    version_id: i64,
    state: tauri::State<'_, AppState>,
) -> CommandResult<NoteVersion> {
    let store = state.store.clone();
    blocking(store, move |store| store.get_version(version_id)).await
}

/// Rolls a note back to a version; the current state is saved first.
#[tauri::command]
pub async fn restore_version(
    version_id: i64,
    state: tauri::State<'_, AppState>,
) -> CommandResult<Note> {
    let store = state.store.clone();
    blocking(store, move |store| store.restore_version(version_id)).await
}

/// Deletes a single version.
#[tauri::command]
pub async fn delete_version(
    version_id: i64,
    state: tauri::State<'_, AppState>,
) -> CommandResult<u32> {
    let store = state.store.clone();
    blocking(store, move |store| store.delete_version(version_id)).await
}

/// Applies the retention policy from settings.
#[tauri::command]
pub async fn prune_versions(state: tauri::State<'_, AppState>) -> CommandResult<u32> {
    let store = state.store.clone();
    let settings = state.settings().versions;
    blocking(store, move |store| store.prune_versions(&settings)).await
}
