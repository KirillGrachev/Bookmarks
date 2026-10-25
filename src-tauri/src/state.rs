//! Application state shared by all commands.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use bookmarks_core::db::Store;
use bookmarks_core::domain::Settings;

/// Background sync bookkeeping.
///
/// Kept in atomics rather than a mutex-guarded struct: the UI polls it from the
/// event loop while the sync task writes from a blocking thread, and a lock here
/// would be exactly the kind of contention that freezes window dragging.
#[derive(Debug, Default)]
pub struct SyncState {
    running: AtomicBool,
    last_run_at: AtomicI64,
    last_error: RwLock<Option<String>>,
}

impl SyncState {
    /// Marks sync as started; returns false if a sync is already running.
    pub fn begin(&self) -> bool {
        !self.running.swap(true, Ordering::SeqCst)
    }

    /// Marks sync as finished.
    pub fn finish(&self, error: Option<String>) {
        self.last_run_at.store(
            bookmarks_core::time::Timestamp::now().millis(),
            Ordering::SeqCst,
        );
        if let Ok(mut guard) = self.last_error.write() {
            *guard = error;
        }
        self.running.store(false, Ordering::SeqCst);
    }

    /// Whether a sync is in progress.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Snapshot for the UI.
    pub fn snapshot(&self) -> SyncStatus {
        let last_run_at = self.last_run_at.load(Ordering::SeqCst);
        SyncStatus {
            running: self.is_running(),
            last_run_at: if last_run_at == 0 {
                None
            } else {
                Some(last_run_at)
            },
            last_error: self.last_error.read().ok().and_then(|guard| guard.clone()),
        }
    }
}

/// Sync status as seen by the frontend.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    /// A sync is in progress.
    pub running: bool,
    /// Last successful or failed run, ms since epoch.
    pub last_run_at: Option<i64>,
    /// Last error message, if any.
    pub last_error: Option<String>,
}

/// Everything commands need, behind one managed handle.
#[derive(Debug)]
pub struct AppState {
    /// Notes storage.
    pub store: Store,
    /// Current settings (written through to disk on change).
    pub settings: RwLock<Settings>,
    /// Settings file path.
    pub settings_path: PathBuf,
    /// Database file path.
    pub database_path: PathBuf,
    /// Application data directory (attachments, backups).
    pub data_dir: PathBuf,
    /// This device's stable identifier.
    pub device_id: String,
    /// Human-readable device name.
    pub device_name: RwLock<String>,
    /// Background sync bookkeeping.
    pub sync: Arc<SyncState>,
    /// Whether the initial startup migration/probe finished.
    pub ready: AtomicBool,
    /// Last known window geometry, persisted by the window-state plugin.
    pub last_error: Mutex<Option<String>>,
}

impl AppState {
    /// Current settings snapshot.
    pub fn settings(&self) -> Settings {
        self.settings
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Replaces settings and persists them.
    pub fn replace_settings(&self, settings: Settings) -> crate::error::CommandResult<Settings> {
        let mut clamped = settings;
        clamped.clamp();
        crate::settings::save(&self.settings_path, &clamped)?;
        if let Ok(mut guard) = self.settings.write() {
            *guard = clamped.clone();
        }
        Ok(clamped)
    }

    /// Device name for manifests and conflict messages.
    pub fn device_name(&self) -> String {
        self.device_name
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| "device".to_owned())
    }
}

/// Type alias used across command signatures.
pub type SharedState = Arc<AppState>;
