//! Logging.
//!
//! Logs go to a file in the platform log directory, not to stdout: a packaged
//! Windows app has no console, and `println!` output would be lost exactly when
//! it is needed most — a user reporting a crash.

use std::fs;
use std::path::Path;

use tauri::{AppHandle, Manager};
use tracing_subscriber::prelude::*;

/// Log file name.
pub const FILE_NAME: &str = "bookmarks.log";

/// Maximum log file size before rotation, in bytes.
const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// Initializes the global subscriber.
///
/// Level is `debug` in development and `info` in release: tracing every IPC call
/// is useful while iterating and is noise in a shipped build.
pub fn init(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let dir = app.path().app_log_dir()?;
    fs::create_dir_all(&dir)?;
    let path = dir.join(FILE_NAME);
    rotate_if_large(&path);

    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;

    let level = if cfg!(debug_assertions) {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };

    // Timestamps come from chrono in UTC: `time::UtcOffset::current_local_offset`
    // fails on a multithreaded process, which would silently drop the timer.
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::sync::Mutex::new(file))
        .with_ansi(false)
        .with_target(true)
        .with_timer(UtcTime)
        .with_filter(tracing_subscriber::filter::LevelFilter::from_level(level));

    tracing_subscriber::registry().with(file_layer).init();

    tracing::info!(
        version = %app.package_info().version,
        log = %path.display(),
        "bookmarks started"
    );
    Ok(())
}

/// UTC timestamp for log lines.
#[derive(Debug, Clone, Copy)]
struct UtcTime;

impl tracing_subscriber::fmt::time::FormatTime for UtcTime {
    fn format_time(
        &self,
        writer: &mut tracing_subscriber::fmt::format::Writer<'_>,
    ) -> std::fmt::Result {
        write!(
            writer,
            "{}",
            chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ")
        )
    }
}

/// Renames an oversized log to `.1`, keeping exactly one previous file.
///
/// One generation is enough for support and cannot grow without bound; a full
/// rotation policy would be a dependency for no practical gain here.
fn rotate_if_large(path: &Path) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if metadata.len() < MAX_BYTES {
        return;
    }
    let rotated = path.with_extension("log.1");
    let _ = fs::remove_file(&rotated);
    let _ = fs::rename(path, &rotated);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_only_when_large() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bookmarks.log");
        fs::write(&path, "small").expect("write");
        rotate_if_large(&path);
        assert!(path.exists(), "small log must stay in place");

        fs::write(&path, vec![b'x'; (MAX_BYTES + 1) as usize]).expect("write large");
        rotate_if_large(&path);
        assert!(!path.exists(), "large log must be rotated away");
        assert!(path.with_extension("log.1").exists());
    }
}
