//! Device identity.
//!
//! The identifier must be stable across restarts, otherwise every launch would
//! look like a new device to the sync engine: manifests would accumulate and each
//! run would treat the previous one as a remote peer.

use std::fs;
use std::path::Path;

use bookmarks_core::ids::EntityId;

/// File holding this device's identifier.
pub const FILE_NAME: &str = "device-id";

/// Returns the stored identifier, creating one on first run.
///
/// `existing` wins when present: the identifier is also mirrored into settings so
/// the UI can show it, and a value there must not be contradicted by a fresh file.
pub fn ensure(config_dir: &Path, existing: Option<&str>) -> Result<EntityId, std::io::Error> {
    if let Some(value) = existing {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_owned());
        }
    }

    let path = config_dir.join(FILE_NAME);
    if let Ok(raw) = fs::read_to_string(&path) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_owned());
        }
    }

    let generated = bookmarks_core::ids::new_id();
    // A failed write is not fatal: sync still works, it will just look like a new
    // device on the next launch, which is visible in the device list.
    let _ = fs::write(&path, generated.as_bytes());
    Ok(generated)
}

/// Human-readable device name for manifests and conflict messages.
///
/// Falls back to the platform name: an empty string in a conflict message would
/// read as "conflict with ", which is worse than a generic label.
pub fn name() -> String {
    hostname().unwrap_or_else(default_name)
}

fn hostname() -> Option<String> {
    for variable in ["COMPUTERNAME", "HOSTNAME"] {
        if let Ok(value) = std::env::var(variable) {
            let trimmed = value.trim().to_owned();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    // POSIX fallback: `hostname` is present on Linux and macOS.
    let output = std::process::Command::new("hostname").output().ok()?;
    if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

fn default_name() -> String {
    match std::env::consts::OS {
        "windows" => "Windows PC".to_owned(),
        "macos" => "Mac".to_owned(),
        "linux" => "Linux machine".to_owned(),
        other => format!("{other} device"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_and_reuses_identifier() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = ensure(dir.path(), None).expect("first");
        let second = ensure(dir.path(), None).expect("second");
        assert_eq!(first, second, "identifier must be stable");
        assert!(bookmarks_core::ids::is_valid_id(&first));
    }

    #[test]
    fn existing_value_wins() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stored = ensure(dir.path(), None).expect("stored");
        let explicit = ensure(dir.path(), Some("explicit-id")).expect("explicit");
        assert_eq!(explicit, "explicit-id");
        // A blank value is ignored, not stored.
        let blank = ensure(dir.path(), Some("   ")).expect("blank");
        assert_eq!(blank, stored);
    }

    #[test]
    fn name_is_never_empty() {
        assert!(!name().trim().is_empty());
        assert!(!default_name().is_empty());
    }
}
