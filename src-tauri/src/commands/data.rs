//! Statistics, maintenance and settings commands.

use bookmarks_core::db::{LibraryStats, StorageStats};
use bookmarks_core::domain::Settings;

use crate::blocking::blocking;
use crate::error::CommandResult;
use crate::state::AppState;

/// Library aggregates for the status line and the About screen.
#[tauri::command]
pub async fn library_stats(state: tauri::State<'_, AppState>) -> CommandResult<LibraryStats> {
    let store = state.store.clone();
    blocking(store, move |store| store.library_stats()).await
}

/// Storage-level statistics, including the database file size.
#[tauri::command]
pub async fn storage_stats(state: tauri::State<'_, AppState>) -> CommandResult<StorageStats> {
    let store = state.store.clone();
    blocking(store, move |store| store.storage_stats()).await
}

/// Runs `integrity_check` and the FTS5 integrity check; empty means healthy.
#[tauri::command]
pub async fn verify_integrity(state: tauri::State<'_, AppState>) -> CommandResult<Vec<String>> {
    let store = state.store.clone();
    blocking(store, move |store| store.verify_integrity()).await
}

/// Rebuilds the full-text index from the notes table.
#[tauri::command]
pub async fn rebuild_search_index(state: tauri::State<'_, AppState>) -> CommandResult<u32> {
    let store = state.store.clone();
    blocking(store, move |store| store.rebuild_search_index()).await
}

/// Recalculates content hashes and word counts for all notes.
#[tauri::command]
pub async fn recompute_derived(state: tauri::State<'_, AppState>) -> CommandResult<u32> {
    let store = state.store.clone();
    blocking(store, move |store| store.recompute_derived_fields()).await
}

/// Returns current settings.
#[tauri::command]
pub fn get_settings(state: tauri::State<'_, AppState>) -> CommandResult<Settings> {
    Ok(state.settings())
}

/// Persists settings and returns the clamped result.
///
/// The clamped copy is what the UI must apply: a hand-edited settings file could
/// ask for `fontSize: 0`, and echoing it back would break the layout.
#[tauri::command]
pub fn save_settings(
    settings: Settings,
    state: tauri::State<'_, AppState>,
) -> CommandResult<Settings> {
    state.replace_settings(settings)
}

/// Restores settings to defaults.
#[tauri::command]
pub fn reset_settings(state: tauri::State<'_, AppState>) -> CommandResult<Settings> {
    state.replace_settings(Settings::default())
}
