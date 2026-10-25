//! Persistent settings storage.
//!
//! Settings live in a JSON file in the app config directory, not in the notes
//! database: they must be readable before the store is opened and must survive a
//! corrupted or encrypted database. Writes go through a temporary file plus
//! rename, so a process killed mid-write cannot leave a half-written settings
//! file behind.

use std::fs;
use std::path::{Path, PathBuf};

use bookmarks_core::domain::Settings;

use crate::error::CommandResult;

/// Settings file name.
pub const FILE_NAME: &str = "settings.json";

/// Resolves the settings path inside a config directory.
pub fn path_in(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE_NAME)
}

/// Loads settings, falling back to defaults for a missing or unreadable file.
///
/// A broken file is preserved next to the new one (`settings.json.broken`) so the
/// user can inspect it, and defaults are returned: refusing to start because of a
/// bad settings file would be worse than starting with sane defaults.
pub fn load(path: &Path) -> CommandResult<Settings> {
    match fs::read_to_string(path) {
        Ok(raw) => match Settings::from_json(&raw) {
            Ok(mut settings) => {
                settings.clamp();
                Ok(settings)
            }
            Err(error) => {
                let backup = path.with_extension("json.broken");
                let _ = fs::rename(path, backup);
                tracing::warn!(%error, "settings file is invalid, restored defaults");
                Ok(Settings::default())
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(error) => {
            tracing::warn!(%error, "settings file is unreadable, using defaults");
            Ok(Settings::default())
        }
    }
}

/// Saves settings atomically.
pub fn save(path: &Path, settings: &Settings) -> CommandResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = settings.to_json()?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, json.as_bytes())?;
    fs::rename(&temporary, path).map_err(|error| {
        // A rename across filesystems fails; fall back to a direct write rather
        // than losing the user's change.
        tracing::warn!(%error, "atomic rename failed, writing settings directly");
        error
    })?;
    Ok(())
}

/// Saves without failing the caller (used on shutdown).
pub fn save_best_effort(path: &Path, settings: &Settings) {
    if let Err(error) = save(path, settings) {
        if let Err(fallback) = fs::write(path, settings.to_json().unwrap_or_default()) {
            tracing::error!(%error, %fallback, "failed to persist settings");
        }
    }
}

#[cfg(test)]
mod tests {
    use bookmarks_core::domain::{AccentColor, Theme};

    use super::*;

    #[test]
    fn missing_file_yields_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = path_in(dir.path());
        let settings = load(&path).expect("load");
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn roundtrip_preserves_values() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = path_in(dir.path());
        let mut settings = Settings::default();
        settings.interface.theme = Theme::Dark;
        settings.interface.accent = AccentColor::Violet;
        settings.editor.font_size = 18;
        save(&path, &settings).expect("save");

        let loaded = load(&path).expect("load");
        assert_eq!(loaded.interface.theme, Theme::Dark);
        assert_eq!(loaded.interface.accent, AccentColor::Violet);
        assert_eq!(loaded.editor.font_size, 18);
    }

    #[test]
    fn broken_file_is_quarantined_and_defaults_returned() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = path_in(dir.path());
        fs::write(&path, "{ definitely not json").expect("write");

        let settings = load(&path).expect("load must not fail");
        assert_eq!(settings, Settings::default());
        assert!(
            path.with_extension("json.broken").exists(),
            "broken file must be kept"
        );
    }

    #[test]
    fn save_creates_missing_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested/config/settings.json");
        save(&path, &Settings::default()).expect("save");
        assert!(path.exists());
    }
}
