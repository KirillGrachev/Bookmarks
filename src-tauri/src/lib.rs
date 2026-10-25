//! Bookmarks — Tauri shell: window, startup tasks and command registration.
//!
//! All substance lives in `bookmarks-core`; this crate only wires it to a window
//! and to system APIs. That separation keeps the core testable without a GUI and
//! lets a mobile Tauri target reuse the same crate.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod blocking;
pub mod commands;
pub mod device;
pub mod error;
pub mod logging;
pub mod settings;
pub mod state;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock};

use bookmarks_core::db::Store;
use bookmarks_core::domain::RemoteKind;
use bookmarks_core::sync::{ConflictPolicy, DirectoryRemote, Remote, SyncEngine};
use tauri::Manager;

use crate::error::CommandError;
use crate::state::{AppState, SyncState};

/// Database file name inside the app data directory.
pub const DATABASE_FILE: &str = "bookmarks.db";

/// Starts the application.
pub fn run() {
    let app = match tauri::Builder::default()
        // A second launch focuses the existing window instead of opening another
        // one: two processes on the same SQLite file would contend for the lock
        // and leave the user guessing which window is authoritative.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup(setup)
        .invoke_handler(register_commands!())
        .build(tauri::generate_context!())
    {
        Ok(app) => app,
        Err(error) => {
            // Without a window there is nowhere to report the failure, so stderr
            // and a non-zero exit code are all that is left.
            eprintln!("[bookmarks] failed to build the application: {error}");
            std::process::exit(1);
        }
    };

    app.run(|app, event| {
        // Persist settings on exit even if the last explicit save failed.
        if let tauri::RunEvent::Exit = event {
            if let Some(state) = app.try_state::<AppState>() {
                let settings = state.settings();
                settings::save_best_effort(&state.settings_path, &settings);
            }
        }
    });
}

/// Opens storage, loads settings and registers application state.
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    logging::init(app.handle())?;

    let data_dir = app.path().app_data_dir()?;
    let config_dir = app.path().app_config_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(&config_dir)?;

    let database_path = data_dir.join(DATABASE_FILE);
    let store = Store::open(&database_path).map_err(|error| {
        // A damaged database must not be silently recreated: that would look like
        // "all my notes disappeared". Report and stop instead.
        format!(
            "cannot open the notes database at {}: {error}",
            database_path.display()
        )
    })?;

    let settings_path = settings::path_in(&config_dir);
    let mut loaded = settings::load(&settings_path).map_err(|error| error.to_string())?;
    let device_id = device::ensure(&config_dir, loaded.sync.device_id.as_deref())?;
    loaded.sync.device_id = Some(device_id.clone());
    settings::save(&settings_path, &loaded)?;

    let device_name = device::name();
    app.manage(AppState {
        store,
        settings: RwLock::new(loaded),
        settings_path,
        database_path,
        data_dir,
        device_id,
        device_name: RwLock::new(device_name),
        sync: Arc::new(SyncState::default()),
        ready: std::sync::atomic::AtomicBool::new(false),
        last_error: Mutex::new(None),
    });

    // Startup work runs in the background: the first frame must not wait on the
    // network or on a large retention cleanup.
    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        startup_tasks(handle).await;
    });

    // The window is created hidden; the frontend shows it once settings are
    // applied, so the user never sees a flash of the default theme.
    Ok(())
}

/// Maintenance that must not delay the first frame.
async fn startup_tasks(app: tauri::AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };

    // Notes in trash older than the retention window are removed on launch. The
    // count is logged rather than shown: a dialog on every start trains the user
    // to click through it without reading.
    let store = state.store.clone();
    match blocking::blocking_task(move || {
        store
            .purge_expired_trash(TRASH_RETENTION_DAYS)
            .map_err(CommandError::from)
    })
    .await
    {
        Ok(removed) if removed > 0 => tracing::info!(removed, "purged expired notes from trash"),
        Ok(_) => {}
        Err(error) => tracing::warn!(?error, "trash cleanup failed"),
    }

    // Version history is pruned by the configured policy for the same reason.
    let store = state.store.clone();
    let policy = state.settings().versions;
    match blocking::blocking_task(move || store.prune_versions(&policy).map_err(CommandError::from))
        .await
    {
        Ok(removed) if removed > 0 => {
            tracing::info!(removed, "pruned versions by retention policy")
        }
        Ok(_) => {}
        Err(error) => tracing::warn!(?error, "version pruning failed"),
    }

    let settings = state.settings();
    if settings.sync.sync_on_start && settings.sync.remote != RemoteKind::None {
        run_startup_sync(&app, &settings.sync).await;
    }

    state.ready.store(true, Ordering::SeqCst);
}

/// Trash retention window, in days.
pub const TRASH_RETENTION_DAYS: u32 = 30;

/// One sync attempt at startup; failures go to the log, not to a dialog.
async fn run_startup_sync(app: &tauri::AppHandle, sync: &bookmarks_core::domain::SyncSettings) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    if !state.sync.begin() {
        return;
    }

    let store = state.store.clone();
    let location = sync.location.clone();
    let device_id = state.device_id.clone();
    let device_name = state.device_name();
    let encrypt_remote = sync.encrypt_remote;

    let result = blocking::blocking_task(move || {
        let remote: Arc<dyn Remote> =
            Arc::new(DirectoryRemote::new(location.trim()).map_err(|error| {
                CommandError::new(bookmarks_core::Error::invalid(error.to_string()).into())
            })?);
        let engine = SyncEngine::new(
            store.clone(),
            remote,
            device_id,
            device_name,
            ConflictPolicy::KeepNewest,
        );
        let engine = if encrypt_remote {
            match store.vault_current_key().map_err(CommandError::from)? {
                Some(key) => engine.with_encryption(key),
                // E2EE must not silently degrade to plaintext because the vault
                // is locked: skip the run and let the user unlock it.
                None => {
                    return Err(CommandError::new(
                        bookmarks_core::Error::vault(
                            "startup sync skipped: vault is locked but remote encryption is on",
                            bookmarks_core::error::VaultErrorCode::BadPassword,
                        )
                        .into(),
                    ));
                }
            }
        } else {
            engine
        };
        engine.run().map_err(CommandError::from)
    })
    .await;

    match result {
        Ok(outcome) => {
            tracing::info!(
                uploaded = outcome.uploaded,
                downloaded = outcome.downloaded,
                conflicts = outcome.conflicts_resolved,
                "startup sync finished"
            );
            state.sync.finish(None);
        }
        Err(error) => {
            tracing::warn!(?error, "startup sync failed");
            state.sync.finish(Some(format!("{error:?}")));
        }
    }
}
