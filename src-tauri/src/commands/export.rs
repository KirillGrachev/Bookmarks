//! Export, import and file dialog commands.

use std::path::PathBuf;

use bookmarks_core::export::{
    ExportFormat, ExportResult, ImportConflictPolicy, ImportOptions, ImportReport,
};
use bookmarks_core::ids::EntityId;
use serde::{Deserialize, Serialize};

use crate::blocking::blocking;
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

/// One file to write to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileToSave {
    /// Relative path inside the target directory.
    pub path: String,
    /// Text content.
    pub content: String,
}

/// Exports the library in the chosen format.
///
/// The command returns file contents rather than writing them: choosing a
/// destination involves a dialog and platform permissions, which belong to the
/// shell, while the core stays portable and testable.
#[tauri::command]
pub async fn export_library(
    format: ExportFormat,
    options: bookmarks_core::export::ExportOptions,
    state: tauri::State<'_, AppState>,
) -> CommandResult<ExportResult> {
    let store = state.store.clone();
    blocking(store, move |store| store.export(format, &options)).await
}

/// Writes exported files under a chosen directory.
///
/// Paths are validated the same way the sync transport validates remote paths:
/// a `../` or an absolute path in the export would write outside the chosen
/// folder, and export files can come from an imported bundle.
#[tauri::command]
pub async fn save_files(files: Vec<FileToSave>, root: PathBuf) -> CommandResult<String> {
    crate::blocking::blocking_task(move || {
        for file in &files {
            let target = resolve_under_root(&root, &file.path)?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, file.content.as_bytes())?;
        }
        Ok(root.to_string_lossy().into_owned())
    })
    .await
}

fn resolve_under_root(root: &std::path::Path, relative: &str) -> CommandResult<PathBuf> {
    let mut resolved = root.to_path_buf();
    for component in std::path::Path::new(relative).components() {
        match component {
            std::path::Component::Normal(part) => resolved.push(part),
            std::path::Component::CurDir => {}
            _ => {
                return Err(CommandError::new(
                    bookmarks_core::Error::invalid(format!("invalid export path: {relative}"))
                        .into(),
                ));
            }
        }
    }
    if !resolved.starts_with(root) {
        return Err(CommandError::new(
            bookmarks_core::Error::invalid(format!(
                "export path escapes the target folder: {relative}"
            ))
            .into(),
        ));
    }
    Ok(resolved)
}

/// Imports a JSON backup bundle.
#[tauri::command]
pub async fn import_bundle(
    raw: String,
    options: ImportOptions,
    state: tauri::State<'_, AppState>,
) -> CommandResult<ImportReport> {
    let store = state.store.clone();
    blocking(store, move |store| store.import_bundle(&raw, &options)).await
}

/// Imports Markdown files read by the frontend.
#[tauri::command]
pub async fn import_markdown_files(
    files: Vec<(String, String)>,
    options: ImportOptions,
    state: tauri::State<'_, AppState>,
) -> CommandResult<ImportReport> {
    let store = state.store.clone();
    blocking(store, move |store| {
        store.import_markdown_files(&files, &options)
    })
    .await
}

/// Opens a path with the system handler (file manager, browser).
#[tauri::command]
pub async fn open_path(app: tauri::AppHandle, path: String) -> CommandResult<()> {
    use tauri_plugin_opener::OpenerExt;

    // Going through the opener plugin keeps `file://` handling and Windows
    // quoting consistent across platforms.
    app.opener().open_path(path, None::<&str>).map_err(|error| {
        CommandError::new(bookmarks_core::Error::invalid(error.to_string()).into())
    })
}

/// Re-exported so the frontend contract stays in one place.
pub type ImportPolicyDto = ImportConflictPolicy;
pub type NoteIdDto = EntityId;
