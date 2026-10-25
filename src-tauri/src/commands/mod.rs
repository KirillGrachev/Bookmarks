//! IPC command modules.
//!
//! Split by domain so a change in one area does not touch the others. Every
//! command is thin: it validates nothing (the core does), converts types where
//! the wire format differs, and runs the work on the blocking pool.

pub mod app;
pub mod data;
pub mod export;
pub mod folders;
pub mod notes;
pub mod sync;
pub mod tags;
pub mod vault;
pub mod versions;

/// Registers every command with the Tauri builder.
///
/// Kept as a macro instead of a hand-written list in `lib.rs`: the list is long,
/// and a missing entry there surfaces as a runtime "command not found" in the
/// webview rather than a compile error.
#[macro_export]
macro_rules! register_commands {
    () => {{
        use $crate::commands::{app, data, export, folders, notes, sync, tags, vault, versions};
        tauri::generate_handler![
            // notes
            notes::list_notes,
            notes::search_notes,
            notes::get_note,
            notes::get_note_body,
            notes::create_note,
            notes::update_note,
            notes::set_note_pinned,
            notes::set_note_archived,
            notes::move_notes,
            notes::delete_notes,
            notes::restore_notes,
            notes::purge_notes,
            notes::empty_trash,
            notes::duplicate_note,
            notes::find_duplicates,
            notes::trash_expired,
            // folders
            folders::list_folders,
            folders::folder_tree,
            folders::create_folder,
            folders::update_folder,
            folders::delete_folder,
            folders::reorder_folders,
            // tags
            tags::list_tags,
            tags::create_tag,
            tags::rename_tag,
            tags::set_tag_color,
            tags::delete_tag,
            tags::add_tag_to_note,
            tags::remove_tag_from_note,
            // versions
            versions::list_versions,
            versions::get_version,
            versions::restore_version,
            versions::delete_version,
            versions::prune_versions,
            // vault
            vault::vault_status,
            vault::vault_initialize,
            vault::vault_unlock,
            vault::vault_lock,
            vault::vault_change_password,
            vault::vault_disable,
            vault::encrypt_note,
            vault::decrypt_note,
            // export and import
            export::export_library,
            export::import_bundle,
            export::import_markdown_files,
            export::save_files,
            export::open_path,
            // sync
            sync::sync_status,
            sync::sync_plan,
            sync::sync_run,
            sync::sync_probe,
            sync::sync_devices,
            // data and maintenance
            data::library_stats,
            data::storage_stats,
            data::verify_integrity,
            data::rebuild_search_index,
            data::recompute_derived,
            data::get_settings,
            data::save_settings,
            data::reset_settings,
            // app
            app::app_info,
        ]
    }};
}
