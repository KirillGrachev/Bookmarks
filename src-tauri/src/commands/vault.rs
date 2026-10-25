//! Vault commands.
//!
//! Passwords are handled here and never logged. The derived key stays in the
//! store's in-memory session (`bookmarks_core::crypto::VaultSession`) and is
//! zeroed on lock and on process exit.

use bookmarks_core::db::VaultStatus;
use bookmarks_core::ids::EntityId;

use crate::blocking::blocking;
use crate::error::CommandResult;
use crate::state::AppState;

/// Whether the vault exists and whether it is currently unlocked.
#[tauri::command]
pub async fn vault_status(state: tauri::State<'_, AppState>) -> CommandResult<VaultStatus> {
    let store = state.store.clone();
    blocking(store, move |store| store.vault_status()).await
}

/// Creates the vault with a new password.
#[tauri::command]
pub async fn vault_initialize(
    password: String,
    state: tauri::State<'_, AppState>,
) -> CommandResult<()> {
    let store = state.store.clone();
    // The password is moved into the task and dropped there: it is not logged
    // and does not outlive the call.
    blocking(store, move |store| store.vault_initialize(&password)).await
}

/// Unlocks the vault, caching the session key.
#[tauri::command]
pub async fn vault_unlock(
    password: String,
    state: tauri::State<'_, AppState>,
) -> CommandResult<()> {
    let store = state.store.clone();
    blocking(store, move |store| {
        store.vault_unlock(&password).map(|_| ())
    })
    .await
}

/// Locks the vault: the session key is zeroed.
#[tauri::command]
pub async fn vault_lock(state: tauri::State<'_, AppState>) -> CommandResult<()> {
    let store = state.store.clone();
    blocking(store, move |store| {
        store.vault_lock();
        Ok(())
    })
    .await
}

/// Changes the vault password, re-encrypting every protected note.
#[tauri::command]
pub async fn vault_change_password(
    current: String,
    next: String,
    state: tauri::State<'_, AppState>,
) -> CommandResult<u32> {
    let store = state.store.clone();
    blocking(store, move |store| {
        store.vault_change_password(&current, &next)
    })
    .await
}

/// Disables the vault, decrypting all protected notes.
#[tauri::command]
pub async fn vault_disable(
    password: String,
    state: tauri::State<'_, AppState>,
) -> CommandResult<u32> {
    let store = state.store.clone();
    blocking(store, move |store| store.vault_disable(&password)).await
}

/// Moves a note into the vault.
#[tauri::command]
pub async fn encrypt_note(id: EntityId, state: tauri::State<'_, AppState>) -> CommandResult<()> {
    let store = state.store.clone();
    blocking(store, move |store| store.encrypt_note(&id)).await
}

/// Moves a note out of the vault.
#[tauri::command]
pub async fn decrypt_note(id: EntityId, state: tauri::State<'_, AppState>) -> CommandResult<()> {
    let store = state.store.clone();
    blocking(store, move |store| store.decrypt_note(&id)).await
}
