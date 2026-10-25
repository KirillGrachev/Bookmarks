//! Folder commands.

use bookmarks_core::db::{DeleteFolderOutcome, FolderNode};
use bookmarks_core::domain::{Folder, FolderDraft};
use bookmarks_core::ids::EntityId;

use crate::blocking::blocking;
use crate::error::CommandResult;
use crate::state::AppState;

/// Folder patch as it crosses IPC.
///
/// `parent_id` uses the same convention as the note patch: reparenting goes
/// through [`update_folder`] with an explicit value, and `null` lifts the folder
/// to the root. There is no "leave unchanged" here because the UI always sends
/// the intended parent when it moves a folder.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderPatchDto {
    /// New name.
    pub name: Option<String>,
    /// New parent; `null` moves to the root.
    pub parent_id: Option<Option<EntityId>>,
    /// New color; `null` clears it.
    pub color: Option<Option<String>>,
    /// New icon; `null` clears it.
    pub icon: Option<Option<String>>,
    /// New position among siblings.
    pub sort_order: Option<i32>,
}

/// Lists folders with note counts.
#[tauri::command]
pub async fn list_folders(state: tauri::State<'_, AppState>) -> CommandResult<Vec<Folder>> {
    let store = state.store.clone();
    blocking(store, move |store| store.list_folders()).await
}

/// Returns the folder tree with full paths.
#[tauri::command]
pub async fn folder_tree(state: tauri::State<'_, AppState>) -> CommandResult<Vec<FolderNode>> {
    let store = state.store.clone();
    blocking(store, move |store| store.folder_tree()).await
}

/// Creates a folder.
#[tauri::command]
pub async fn create_folder(
    draft: FolderDraft,
    state: tauri::State<'_, AppState>,
) -> CommandResult<Folder> {
    let store = state.store.clone();
    blocking(store, move |store| store.create_folder(draft)).await
}

/// Updates a folder; rejects moves that would create a cycle.
#[tauri::command]
pub async fn update_folder(
    id: EntityId,
    patch: FolderPatchDto,
    state: tauri::State<'_, AppState>,
) -> CommandResult<Folder> {
    let store = state.store.clone();
    blocking(store, move |store| {
        store.update_folder(
            &id,
            bookmarks_core::domain::FolderPatch {
                name: patch.name,
                parent_id: patch.parent_id,
                color: patch.color,
                icon: patch.icon,
                sort_order: patch.sort_order,
            },
        )
    })
    .await
}

/// Deletes a folder, lifting its notes to the root and its children one level up.
#[tauri::command]
pub async fn delete_folder(
    id: EntityId,
    state: tauri::State<'_, AppState>,
) -> CommandResult<DeleteFolderOutcome> {
    let store = state.store.clone();
    blocking(store, move |store| store.delete_folder(&id)).await
}

/// Applies a new order of folders among siblings.
#[tauri::command]
pub async fn reorder_folders(
    ids: Vec<EntityId>,
    state: tauri::State<'_, AppState>,
) -> CommandResult<()> {
    let store = state.store.clone();
    blocking(store, move |store| store.reorder_folders(&ids)).await
}
