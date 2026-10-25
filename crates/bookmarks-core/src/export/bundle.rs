//! Экспорт и импорт JSON-связки — полный слепок библиотеки.
//!
//! Один файл содержит папки, теги, заметки и агрегаты: этого достаточно для
//! резервной копии и для переноса на другое устройство. Формат версионируется
//! (`magic` + `version`), поэтому файл от будущей версии приложения будет
//! отвергнут целиком, а не прочитан наполовину.

use serde::{Deserialize, Serialize};

use crate::db::{LibraryStats, Store};
use crate::domain::Folder;
use crate::error::{Error, Result};
use crate::export::{ExportNote, ExportedFile, ImportConflictPolicy, ImportOptions, ImportReport};
use crate::ids::EntityId;
use crate::time::Timestamp;

/// Магический префикс связки.
pub const BUNDLE_MAGIC: &str = "BKBNDL1";

/// Текущая версия формата связки.
pub const BUNDLE_VERSION: u32 = 1;

/// Имя файла связки по умолчанию.
pub const DEFAULT_FILE_NAME: &str = "bookmarks-backup.json";

/// Папка в связке.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleFolder {
    /// Идентификатор.
    pub id: EntityId,
    /// Название.
    pub name: String,
    /// Родитель.
    pub parent_id: Option<EntityId>,
    /// Цвет.
    pub color: Option<String>,
    /// Иконка.
    pub icon: Option<String>,
    /// Порядок.
    pub sort_order: i32,
}

/// Тег в связке.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleTag {
    /// Идентификатор.
    pub id: EntityId,
    /// Название.
    pub name: String,
    /// Цвет.
    pub color: Option<String>,
}

/// Полный файл связки.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupBundle {
    /// Маркер формата.
    pub magic: String,
    /// Версия формата.
    pub version: u32,
    /// Версия приложения, создавшего файл.
    pub app_version: String,
    /// Момент создания.
    pub exported_at: Timestamp,
    /// Папки.
    pub folders: Vec<BundleFolder>,
    /// Теги.
    pub tags: Vec<BundleTag>,
    /// Заметки.
    pub notes: Vec<ExportNote>,
    /// Агрегаты на момент выгрузки — для быстрой проверки целостности.
    pub stats: LibraryStats,
}

impl BackupBundle {
    /// Проверяет маркер и версию формата.
    pub fn validate(&self) -> Result<()> {
        if self.magic != BUNDLE_MAGIC {
            return Err(Error::invalid(format!(
                "файл не является резервной копией Bookmarks (magic = {})",
                self.magic
            )));
        }
        if self.version > BUNDLE_VERSION {
            return Err(Error::invalid(format!(
                "копия создана версией формата {} — обновите приложение (поддерживается {})",
                self.version, BUNDLE_VERSION
            )));
        }
        Ok(())
    }
}

/// Собирает файл связки.
pub fn render(
    notes: &[ExportNote],
    folders: &[Folder],
    tags: &[crate::domain::Tag],
    stats: LibraryStats,
) -> Result<ExportedFile> {
    let bundle = BackupBundle {
        magic: BUNDLE_MAGIC.to_owned(),
        version: BUNDLE_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        exported_at: Timestamp::now(),
        folders: folders
            .iter()
            .map(|folder| BundleFolder {
                id: folder.id.clone(),
                name: folder.name.clone(),
                parent_id: folder.parent_id.clone(),
                color: folder.color.clone(),
                icon: folder.icon.clone(),
                sort_order: folder.sort_order,
            })
            .collect(),
        tags: tags
            .iter()
            .map(|tag| BundleTag {
                id: tag.id.clone(),
                name: tag.name.clone(),
                color: tag.color.clone(),
            })
            .collect(),
        notes: notes.to_vec(),
        stats,
    };
    Ok(ExportedFile {
        path: DEFAULT_FILE_NAME.to_owned(),
        content: serde_json::to_string_pretty(&bundle)?,
    })
}

/// Импортирует связку.
///
/// Порядок действий: сначала папки (чтобы заметки было куда класть), затем
/// заметки. Существующие записи находятся по идентификатору, а если его нет —
/// по хешу содержимого, поэтому повторный импорт той же копии не плодит
/// дубликаты.
pub fn import(store: &Store, raw: &str, options: &ImportOptions) -> Result<ImportReport> {
    let bundle: BackupBundle = serde_json::from_str(raw)?;
    bundle.validate()?;

    let mut report = ImportReport::default();
    let tags_before = store.list_tags()?.len() as u32;

    // Карта «идентификатор папки в файле → идентификатор в этой базе»:
    // при переносе между устройствами внутренние ключи не совпадают.
    let mut folder_map: std::collections::HashMap<EntityId, EntityId> =
        std::collections::HashMap::new();
    if options.create_folders {
        let mut ordered = bundle.folders.clone();
        ordered.sort_by_key(|folder| depth_of(folder, &bundle.folders));
        for folder in ordered {
            let parent_id = folder
                .parent_id
                .as_ref()
                .and_then(|parent| folder_map.get(parent))
                .cloned()
                .or_else(|| options.folder_id.clone());
            match store.ensure_folder(&folder.name, parent_id.as_deref(), folder.color.as_deref()) {
                Ok(id) => {
                    folder_map.insert(folder.id.clone(), id.clone());
                    report.folders_created += 1;
                }
                Err(error) => report
                    .errors
                    .push(format!("папка «{}»: {error}", folder.name)),
            }
        }
    }

    for note in &bundle.notes {
        let folder_id = note
            .folder_id
            .as_ref()
            .and_then(|id| folder_map.get(id))
            .cloned()
            .or_else(|| options.folder_id.clone());
        let hash = if note.content_hash.is_empty() {
            crate::domain::note::content_hash(&note.title, &note.body)
        } else {
            note.content_hash.clone()
        };

        if let Some(existing) = store.find_note_by_hash(&hash, Some(note.id.as_str()))? {
            match options.conflict_policy {
                ImportConflictPolicy::Skip => {
                    report.skipped += 1;
                    continue;
                }
                ImportConflictPolicy::Overwrite => {
                    store.update_note(
                        &existing,
                        crate::domain::note::NotePatch {
                            title: Some(note.title.clone()),
                            body: Some(note.body.clone()),
                            folder_id: Some(folder_id),
                            tags: Some(note.tags.clone()),
                            pinned: Some(note.pinned),
                            archived: Some(note.archived),
                            color: Some(note.color.clone()),
                        },
                    )?;
                    report.updated += 1;
                    continue;
                }
                ImportConflictPolicy::Duplicate => {}
            }
        }

        store.create_note(crate::domain::note::NoteDraft {
            title: Some(note.title.clone()),
            body: Some(note.body.clone()),
            folder_id,
            tags: note.tags.clone(),
            pinned: Some(note.pinned),
            color: note.color.clone(),
            encrypted: Some(false),
        })?;
        report.created += 1;
    }

    let tags_after = store.list_tags()?.len() as u32;
    report.tags_created = tags_after.saturating_sub(tags_before);
    Ok(report)
}

/// Глубина папки в дереве связки (для сортировки «родители раньше потомков»).
fn depth_of(folder: &BundleFolder, folders: &[BundleFolder]) -> u32 {
    let mut depth = 0u32;
    let mut current = folder.parent_id.clone();
    while let Some(parent_id) = current {
        depth += 1;
        if depth > crate::domain::limits::MAX_FOLDER_DEPTH {
            break;
        }
        current = folders
            .iter()
            .find(|candidate| candidate.id == parent_id)
            .and_then(|candidate| candidate.parent_id.clone());
    }
    depth
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_note(title: &str, body: &str) -> ExportNote {
        ExportNote {
            id: crate::ids::new_id(),
            title: title.to_owned(),
            body: body.to_owned(),
            folder_id: None,
            folder_path: String::new(),
            tags: vec!["идеи".into()],
            pinned: false,
            archived: false,
            color: None,
            encrypted: false,
            created_at: Timestamp::now(),
            updated_at: Timestamp::now(),
            content_hash: String::new(),
        }
    }

    fn sample_bundle(notes: Vec<ExportNote>) -> BackupBundle {
        BackupBundle {
            magic: BUNDLE_MAGIC.to_owned(),
            version: BUNDLE_VERSION,
            app_version: "test".into(),
            exported_at: Timestamp::now(),
            folders: vec![BundleFolder {
                id: "folder-1".into(),
                name: "Работа".into(),
                parent_id: None,
                color: Some("blue".into()),
                icon: None,
                sort_order: 0,
            }],
            tags: vec![BundleTag {
                id: "tag-1".into(),
                name: "идеи".into(),
                color: None,
            }],
            notes,
            stats: LibraryStats::default(),
        }
    }

    #[test]
    fn render_produces_single_json_file() {
        let file = render(
            &[sample_note("Первая", "текст")],
            &[],
            &[],
            LibraryStats::default(),
        )
        .expect("render");
        assert_eq!(file.path, DEFAULT_FILE_NAME);
        let bundle: BackupBundle = serde_json::from_str(&file.content).expect("valid json");
        assert_eq!(bundle.magic, BUNDLE_MAGIC);
        assert_eq!(bundle.notes.len(), 1);
        assert!(!bundle.app_version.is_empty());
    }

    #[test]
    fn validate_rejects_foreign_and_future_files() {
        let mut foreign = sample_bundle(vec![]);
        foreign.magic = "ZIPZIP1".into();
        assert!(foreign.validate().is_err());

        let mut future = sample_bundle(vec![]);
        future.version = BUNDLE_VERSION + 1;
        let error = future.validate().expect_err("must fail");
        assert!(error.to_string().contains("обновите приложение"));
    }

    #[test]
    fn import_creates_notes_folders_and_tags() {
        let store = Store::open_in_memory().expect("store");
        let mut note = sample_note("Импортированная", "тело");
        note.folder_id = Some("folder-1".into());
        let raw = serde_json::to_string(&sample_bundle(vec![note])).expect("json");

        let report = store
            .import_bundle(&raw, &ImportOptions::default())
            .expect("import");
        assert_eq!(report.created, 1);
        assert_eq!(report.folders_created, 1);
        assert_eq!(report.tags_created, 1);
        assert!(store
            .list_folders()
            .expect("folders")
            .iter()
            .any(|folder| folder.name == "Работа"));
        let imported = store.list_notes(&Default::default()).expect("notes");
        assert_eq!(imported.items.len(), 1);
        assert_eq!(imported.items[0].tags, vec!["идеи".to_string()]);
    }

    #[test]
    fn second_import_skips_duplicates() {
        let store = Store::open_in_memory().expect("store");
        let raw =
            serde_json::to_string(&sample_bundle(vec![sample_note("Одна", "тело")])).expect("json");

        let first = store
            .import_bundle(&raw, &ImportOptions::default())
            .expect("first");
        let second = store
            .import_bundle(&raw, &ImportOptions::default())
            .expect("second");
        assert_eq!(first.created, 1);
        assert_eq!(second.created, 0);
        assert_eq!(second.skipped, 1);
        assert_eq!(store.library_stats().expect("stats").notes, 1);
    }

    #[test]
    fn overwrite_policy_replaces_existing_note() {
        let store = Store::open_in_memory().expect("store");
        let raw = serde_json::to_string(&sample_bundle(vec![sample_note(
            "Заметка",
            "первая версия",
        )]))
        .expect("json");
        store
            .import_bundle(&raw, &ImportOptions::default())
            .expect("first");

        let mut changed = sample_note("Заметка", "вторая версия");
        changed.content_hash = crate::domain::note::content_hash("Заметка", "первая версия");
        let raw_changed = serde_json::to_string(&sample_bundle(vec![changed])).expect("json");

        let options = ImportOptions {
            conflict_policy: ImportConflictPolicy::Overwrite,
            ..Default::default()
        };
        let report = store
            .import_bundle(&raw_changed, &options)
            .expect("overwrite");
        assert_eq!(report.updated, 1);
        assert_eq!(report.created, 0);
        assert_eq!(store.library_stats().expect("stats").notes, 1);
        let notes = store.sync_snapshot().expect("snapshot");
        assert_eq!(notes[0].body, "вторая версия");
    }

    #[test]
    fn rejects_garbage() {
        let store = Store::open_in_memory().expect("store");
        assert!(store.import_bundle("{", &ImportOptions::default()).is_err());
        assert!(store
            .import_bundle("[]", &ImportOptions::default())
            .is_err());
    }

    #[test]
    fn depth_sorting_puts_parents_first() {
        let folders = vec![
            BundleFolder {
                id: "c".into(),
                name: "Вложенная".into(),
                parent_id: Some("b".into()),
                color: None,
                icon: None,
                sort_order: 0,
            },
            BundleFolder {
                id: "b".into(),
                name: "Средняя".into(),
                parent_id: Some("a".into()),
                color: None,
                icon: None,
                sort_order: 0,
            },
            BundleFolder {
                id: "a".into(),
                name: "Корень".into(),
                parent_id: None,
                color: None,
                icon: None,
                sort_order: 0,
            },
        ];
        let depths: Vec<u32> = folders
            .iter()
            .map(|folder| depth_of(folder, &folders))
            .collect();
        assert_eq!(depths, vec![2, 1, 0]);
    }
}
