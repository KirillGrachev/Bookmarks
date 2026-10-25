//! Application info command.

use std::path::Path;

use serde::Serialize;

use crate::error::CommandResult;
use crate::state::AppState;

/// Info shown on the About screen.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// Application version from the bundle config.
    pub version: String,
    /// Tauri runtime version.
    pub tauri_version: String,
    /// Platform identifier (`windows`, `linux`, `macos`).
    pub platform: String,
    /// CPU architecture.
    pub arch: String,
    /// Directory holding the database, backups and attachments.
    pub data_dir: String,
    /// Database file path.
    pub database_path: String,
}

/// Returns version, platform and data locations.
#[tauri::command]
pub fn app_info(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> CommandResult<AppInfo> {
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        tauri_version: tauri::VERSION.to_owned(),
        platform: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        data_dir: display(&state.data_dir),
        database_path: display(&state.database_path),
    })
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
