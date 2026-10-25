//! Tag commands.

use bookmarks_core::domain::{Tag, TagDraft};
use bookmarks_core::ids::EntityId;

use crate::blocking::blocking;
use crate::error::CommandResult;
use crate::state::AppState;

/// Lists tags with note counts, most used first.
#[tauri::command]
pub async fn list_tags(state: tauri::State<'_, AppState>) -> CommandResult<Vec<Tag>> {
    let store = state.store.clone();
    blocking(store, move |store| store.list_tags()).await
}

/// Creates a tag.
#[tauri::command]
pub async fn create_tag(draft: TagDraft, state: tauri::State<'_, AppState>) -> CommandResult<()> {
    let store = state.store.clone();
    blocking(store, move |store| store.create_tag(draft)).await
}

/// Renames a tag, merging it with an existing tag of the same name.
#[tauri::command]
pub async fn rename_tag(
    id: EntityId,
    name: String,
    state: tauri::State<'_, AppState>,
) -> CommandResult<()> {
    let store = state.store.clone();
    blocking(store, move |store| store.rename_tag(&id, &name)).await
}

/// Sets or clears a tag color.
#[tauri::command]
pub async fn set_tag_color(
    id: EntityId,
    color: Option<String>,
    state: tauri::State<'_, AppState>,
) -> CommandResult<()> {
    let store = state.store.clone();
    blocking(store, move |store| {
        store.set_tag_color(&id, color.as_deref())
    })
    .await
}

/// Deletes a tag; returns how many notes lost the label.
#[tauri::command]
pub async fn delete_tag(id: EntityId, state: tauri::State<'_, AppState>) -> CommandResult<u32> {
    let store = state.store.clone();
    blocking(store, move |store| store.delete_tag(&id)).await
}

/// Attaches a tag to a note, creating the tag when needed.
#[tauri::command]
pub async fn add_tag_to_note(
    note_id: EntityId,
    tag_name: String,
    state: tauri::State<'_, AppState>,
) -> CommandResult<()> {
    let store = state.store.clone();
    blocking(store, move |store| {
        store.add_tag_to_note(&note_id, &tag_name)
    })
    .await
}

/// Detaches a tag from a note.
#[tauri::command]
pub async fn remove_tag_from_note(
    note_id: EntityId,
    tag_id: EntityId,
    state: tauri::State<'_, AppState>,
) -> CommandResult<()> {
    let store = state.store.clone();
    blocking(store, move |store| {
        store.remove_tag_from_note(&note_id, &tag_id)
    })
    .await
}
