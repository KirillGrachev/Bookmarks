//! Note commands.

use bookmarks_core::db::{NoteFilter, NotePage, NoteQuery};
use bookmarks_core::domain::note::{Note, NoteDraft, NotePatch, NoteSummary};
use bookmarks_core::ids::EntityId;

use crate::blocking::blocking;
use crate::error::CommandResult;
use crate::state::AppState;

/// Patch shape accepted over IPC.
///
/// `folder_id` is deliberately absent: `Option<Option<T>>` cannot be told apart
/// from a missing key once it crosses the JS boundary, and guessing would make
/// "move to root" and "leave the folder alone" indistinguishable. Moving is an
/// explicit [`move_notes`] call instead.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePatchDto {
    /// New title.
    pub title: Option<String>,
    /// New body.
    pub body: Option<String>,
    /// New tag set (replaces the current one).
    pub tags: Option<Vec<String>>,
    /// Pin state.
    pub pinned: Option<bool>,
    /// Archive state.
    pub archived: Option<bool>,
    /// Color label; `null` clears it.
    pub color: Option<Option<String>>,
}

impl From<NotePatchDto> for NotePatch {
    fn from(dto: NotePatchDto) -> Self {
        NotePatch {
            title: dto.title,
            body: dto.body,
            folder_id: None,
            tags: dto.tags,
            pinned: dto.pinned,
            archived: dto.archived,
            color: dto.color,
        }
    }
}

/// Lists notes with filters, sorting and paging.
#[tauri::command]
pub async fn list_notes(
    query: NoteQuery,
    state: tauri::State<'_, AppState>,
) -> CommandResult<NotePage> {
    let store = state.store.clone();
    blocking(store, move |store| store.list_notes(&query)).await
}

/// Full-text search with filters applied.
#[tauri::command]
pub async fn search_notes(
    query: String,
    filter: NoteFilter,
    limit: u32,
    state: tauri::State<'_, AppState>,
) -> CommandResult<Vec<bookmarks_core::db::SearchHit>> {
    let store = state.store.clone();
    blocking(store, move |store| {
        store.search_notes(&query, &filter, limit)
    })
    .await
}

/// Returns a single note with tag names.
#[tauri::command]
pub async fn get_note(id: EntityId, state: tauri::State<'_, AppState>) -> CommandResult<Note> {
    let store = state.store.clone();
    blocking(store, move |store| store.get_note(&id)).await
}

/// Returns the plaintext body, decrypting it when the note is in the vault.
#[tauri::command]
pub async fn get_note_body(
    id: EntityId,
    state: tauri::State<'_, AppState>,
) -> CommandResult<String> {
    let store = state.store.clone();
    blocking(store, move |store| store.read_note_body(&id)).await
}

/// Creates a note.
#[tauri::command]
pub async fn create_note(
    draft: NoteDraft,
    state: tauri::State<'_, AppState>,
) -> CommandResult<Note> {
    let store = state.store.clone();
    blocking(store, move |store| store.create_note(draft)).await
}

/// Updates a note; saves a version when the content changed.
#[tauri::command]
pub async fn update_note(
    id: EntityId,
    patch: NotePatchDto,
    state: tauri::State<'_, AppState>,
) -> CommandResult<Note> {
    let store = state.store.clone();
    blocking(store, move |store| store.update_note(&id, patch.into())).await
}

/// Pins or unpins a note.
#[tauri::command]
pub async fn set_note_pinned(
    id: EntityId,
    pinned: bool,
    state: tauri::State<'_, AppState>,
) -> CommandResult<Note> {
    let store = state.store.clone();
    blocking(store, move |store| store.set_note_pinned(&id, pinned)).await
}

/// Archives or unarchives a note.
#[tauri::command]
pub async fn set_note_archived(
    id: EntityId,
    archived: bool,
    state: tauri::State<'_, AppState>,
) -> CommandResult<Note> {
    let store = state.store.clone();
    blocking(store, move |store| store.set_note_archived(&id, archived)).await
}

/// Moves notes to a folder; `null` moves them to the root.
#[tauri::command]
pub async fn move_notes(
    ids: Vec<EntityId>,
    folder_id: Option<EntityId>,
    state: tauri::State<'_, AppState>,
) -> CommandResult<u32> {
    let store = state.store.clone();
    blocking(store, move |store| {
        store.move_notes(&ids, folder_id.as_deref())
    })
    .await
}

/// Soft-deletes notes (moves them to trash).
#[tauri::command]
pub async fn delete_notes(
    ids: Vec<EntityId>,
    state: tauri::State<'_, AppState>,
) -> CommandResult<u32> {
    let store = state.store.clone();
    blocking(store, move |store| {
        let mut deleted = 0u32;
        for id in &ids {
            store.delete_note(id)?;
            deleted += 1;
        }
        Ok(deleted)
    })
    .await
}

/// Restores notes from trash.
#[tauri::command]
pub async fn restore_notes(
    ids: Vec<EntityId>,
    state: tauri::State<'_, AppState>,
) -> CommandResult<u32> {
    let store = state.store.clone();
    blocking(store, move |store| {
        let mut restored = 0u32;
        for id in &ids {
            store.restore_note(id)?;
            restored += 1;
        }
        Ok(restored)
    })
    .await
}

/// Deletes notes permanently.
#[tauri::command]
pub async fn purge_notes(
    ids: Vec<EntityId>,
    state: tauri::State<'_, AppState>,
) -> CommandResult<u32> {
    let store = state.store.clone();
    blocking(store, move |store| {
        let mut purged = 0u32;
        for id in &ids {
            store.purge_note(id)?;
            purged += 1;
        }
        Ok(purged)
    })
    .await
}

/// Empties the trash.
#[tauri::command]
pub async fn empty_trash(state: tauri::State<'_, AppState>) -> CommandResult<u32> {
    let store = state.store.clone();
    blocking(store, move |store| store.empty_trash()).await
}

/// Duplicates a note.
#[tauri::command]
pub async fn duplicate_note(
    id: EntityId,
    state: tauri::State<'_, AppState>,
) -> CommandResult<Note> {
    let store = state.store.clone();
    blocking(store, move |store| store.duplicate_note(&id)).await
}

/// Groups notes with identical content.
#[tauri::command]
pub async fn find_duplicates(
    state: tauri::State<'_, AppState>,
) -> CommandResult<Vec<bookmarks_core::db::DuplicateGroup>> {
    let store = state.store.clone();
    blocking(store, move |store| store.find_duplicates()).await
}

/// Trash entries older than `days` days, without deleting them.
#[tauri::command]
pub async fn trash_expired(
    days: u32,
    state: tauri::State<'_, AppState>,
) -> CommandResult<Vec<EntityId>> {
    let store = state.store.clone();
    blocking(store, move |store| store.trash_expired_ids(days)).await
}

/// Re-exported for the frontend contract tests.
pub type NoteSummaryDto = NoteSummary;
