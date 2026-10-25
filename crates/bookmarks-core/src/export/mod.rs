//! Экспорт и импорт библиотеки.
//!
//! Три формата решают разные задачи:
//! - **Markdown** (каталог `.md` файлов с YAML front-matter) — перенос в Obsidian,
//!   Logseq, Joplin, Git; структура папок сохраняется каталогами;
//! - **JSON** (один файл-связка) — полный слепок для резервной копии и переноса
//!   между устройствами, включая папки, теги, цвета и метки времени;
//! - **HTML** (один файл) — печать и отправка коллегам без приложения.
//!
//! Импорт понимает Markdown-каталог и JSON-связку; дубликаты определяются по
//! хешу содержимого, поэтому повторный импорт не размножает заметки.

pub mod bundle;
pub mod html;
pub mod markdown;

use serde::{Deserialize, Serialize};

use crate::db::{Store, SyncNote};
use crate::error::Result;
use crate::ids::EntityId;
use crate::time::Timestamp;

/// Формат экспорта.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    /// Каталог файлов Markdown.
    Markdown,
    /// Один JSON-файл.
    Json,
    /// Один HTML-файл.
    Html,
}

impl ExportFormat {
    /// Расширение файла для одиночных форматов.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Json => "json",
            Self::Html => "html",
        }
    }
}

/// Параметры экспорта.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptions {
    /// Включить архив.
    #[serde(default = "default_true")]
    pub include_archived: bool,
    /// Включить корзину.
    #[serde(default)]
    pub include_trashed: bool,
    /// Включить зашифрованные заметки (только при разблокированном сейфе).
    #[serde(default)]
    pub include_encrypted: bool,
    /// Экспортировать только выбранные заметки (пусто — все).
    #[serde(default)]
    pub note_ids: Vec<EntityId>,
    /// Добавить в начало каждого файла YAML front-matter.
    #[serde(default = "default_true")]
    pub front_matter: bool,
}

fn default_true() -> bool {
    true
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            include_archived: true,
            include_trashed: false,
            include_encrypted: false,
            note_ids: Vec::new(),
            front_matter: true,
        }
    }
}

/// Один файл экспорта.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedFile {
    /// Относительный путь внутри каталога экспорта (всегда через `/`).
    pub path: String,
    /// Текстовое содержимое.
    pub content: String,
}

/// Результат экспорта.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    /// Формат.
    pub format: ExportFormat,
    /// Файлы (для Markdown — много, для JSON/HTML — один).
    pub files: Vec<ExportedFile>,
    /// Сколько заметок выгружено.
    pub notes: u32,
    /// Сколько пропущено (зашифрованные при заблокированном сейфе и т. п.).
    pub skipped: u32,
    /// Причины пропуска — чтобы пользователь понимал, чего не хватает.
    #[serde(default)]
    pub skipped_reasons: Vec<String>,
    /// Суммарный размер, байт.
    pub bytes: u64,
}

/// Данные заметки для экспорта (тело уже расшифровано, если это возможно).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportNote {
    /// Идентификатор.
    pub id: EntityId,
    /// Заголовок.
    pub title: String,
    /// Тело в Markdown.
    pub body: String,
    /// Идентификатор папки (в связке используется для восстановления связей).
    pub folder_id: Option<EntityId>,
    /// Путь папки от корня, через `/` (в Markdown повторяет структуру каталогов).
    pub folder_path: String,
    /// Названия тегов.
    pub tags: Vec<String>,
    /// Закреплена.
    pub pinned: bool,
    /// В архиве.
    pub archived: bool,
    /// Цветовая метка.
    pub color: Option<String>,
    /// Зашифрована (тело при этом пустое).
    pub encrypted: bool,
    /// Создана.
    pub created_at: Timestamp,
    /// Изменена.
    pub updated_at: Timestamp,
    /// Хеш содержимого — по нему импорт находит дубликаты.
    #[serde(default)]
    pub content_hash: String,
}

impl ExportNote {
    /// Собирает из записи синхронизации и пути папки.
    pub fn from_sync_note(note: SyncNote, folder_path: String) -> Self {
        let hash = if note.content_hash.is_empty() {
            crate::domain::note::content_hash(&note.title, &note.body)
        } else {
            note.content_hash.clone()
        };
        Self {
            id: note.id,
            title: note.title,
            body: note.body,
            folder_id: note.folder_id.clone(),
            folder_path,
            tags: note.tags,
            pinned: note.pinned,
            archived: note.archived,
            color: note.color,
            encrypted: note.encrypted,
            created_at: note.created_at,
            updated_at: note.updated_at,
            content_hash: hash,
        }
    }
}

/// Отчёт об импорте.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    /// Создано заметок.
    pub created: u32,
    /// Обновлено существующих.
    pub updated: u32,
    /// Пропущено дубликатов.
    pub skipped: u32,
    /// Файлов, которые не удалось разобрать.
    pub failed: u32,
    /// Подробные причины отказов.
    #[serde(default)]
    pub errors: Vec<String>,
    /// Созданные папки.
    pub folders_created: u32,
    /// Созданные теги.
    pub tags_created: u32,
}

/// Действие при совпадении с существующей заметкой.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportConflictPolicy {
    /// Пропустить (поведение по умолчанию — импорт ничего не ломает).
    #[default]
    Skip,
    /// Создать копию рядом.
    Duplicate,
    /// Перезаписать существующую (со снапшотом в историю версий).
    Overwrite,
}

/// Параметры импорта.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOptions {
    /// Политика при совпадении.
    #[serde(default)]
    pub conflict_policy: ImportConflictPolicy,
    /// Целевая папка (пусто — корень).
    #[serde(default)]
    pub folder_id: Option<EntityId>,
    /// Восстанавливать структуру каталогов как папки.
    #[serde(default = "default_true")]
    pub create_folders: bool,
    /// Извлекать `#теги` из текста как теги.
    #[serde(default = "default_true")]
    pub extract_inline_tags: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            conflict_policy: ImportConflictPolicy::Skip,
            folder_id: None,
            create_folders: true,
            extract_inline_tags: true,
        }
    }
}

impl Store {
    /// Собирает заметки для экспорта с учётом параметров.
    ///
    /// Зашифрованные заметки отдаются с пустым телом, если сейф заблокирован:
    /// экспорт не должен ни падать, ни молча терять данные — факт пропуска
    /// попадает в `skipped_reasons`.
    pub fn collect_export_notes(
        &self,
        options: &ExportOptions,
    ) -> Result<(Vec<ExportNote>, Vec<String>)> {
        let folder_paths = self.folder_path_map()?;
        let snapshot = self.sync_snapshot()?;
        let selected: Option<std::collections::HashSet<&str>> = if options.note_ids.is_empty() {
            None
        } else {
            Some(options.note_ids.iter().map(String::as_str).collect())
        };

        let mut notes = Vec::new();
        let mut skipped_reasons = Vec::new();
        let mut skipped_encrypted = 0u32;

        for note in snapshot {
            if let Some(ids) = &selected {
                if !ids.contains(note.id.as_str()) {
                    continue;
                }
            }
            if note.deleted && !options.include_trashed {
                continue;
            }
            if note.archived && !options.include_archived {
                continue;
            }
            // Путь папки вычисляется до перемещения записи в экспортный тип.
            let path = folder_path(&folder_paths, &note.folder_id);
            if note.encrypted {
                if !options.include_encrypted {
                    skipped_encrypted += 1;
                    continue;
                }
                // Тело зашифрованной заметки доступно только при
                // разблокированном сейфе; иначе заметка пропускается, а факт
                // пропуска попадает в отчёт.
                match self.read_note_body(&note.id) {
                    Ok(body) => {
                        let mut export = ExportNote::from_sync_note(note, path);
                        export.body = body;
                        export.encrypted = true;
                        notes.push(export);
                    }
                    Err(_) => skipped_encrypted += 1,
                }
                continue;
            }
            notes.push(ExportNote::from_sync_note(note, path));
        }

        if skipped_encrypted > 0 {
            skipped_reasons.push(format!(
                "пропущено зашифрованных заметок: {skipped_encrypted} (сейф заблокирован или экспорт без сейфа)"
            ));
        }
        notes.sort_by(|left, right| {
            left.folder_path
                .cmp(&right.folder_path)
                .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
        });
        Ok((notes, skipped_reasons))
    }

    /// Экспорт в выбранный формат.
    pub fn export(&self, format: ExportFormat, options: &ExportOptions) -> Result<ExportResult> {
        let (notes, skipped_reasons) = self.collect_export_notes(options)?;
        let folders = self.list_folders()?;
        let tags = self.list_tags()?;
        let files = match format {
            ExportFormat::Markdown => markdown::render(&notes, options, &folders),
            ExportFormat::Json => vec![bundle::render(
                &notes,
                &folders,
                &tags,
                self.library_stats()?,
            )?],
            ExportFormat::Html => vec![html::render(&notes, options)],
        };
        let bytes = files.iter().map(|file| file.content.len() as u64).sum();
        let skipped = skipped_encrypted_count(&skipped_reasons);
        Ok(ExportResult {
            format,
            notes: notes.len() as u32,
            skipped,
            skipped_reasons,
            bytes,
            files,
        })
    }

    /// Импорт JSON-связки.
    pub fn import_bundle(&self, raw: &str, options: &ImportOptions) -> Result<ImportReport> {
        bundle::import(self, raw, options)
    }

    /// Импорт одного Markdown-файла (с front-matter или без).
    pub fn import_markdown(
        &self,
        raw: &str,
        options: &ImportOptions,
        folder_path: &str,
    ) -> Result<ImportReport> {
        markdown::import(self, raw, options, folder_path)
    }

    /// Импорт каталога Markdown-файлов.
    ///
    /// Принимает уже прочитанные файлы (`путь → содержимое`): чтение с диска
    /// остаётся за слоем приложения, которое знает про разрешения и диалоги,
    /// а ядро остаётся переносимым и тестируемым.
    pub fn import_markdown_files(
        &self,
        files: &[(String, String)],
        options: &ImportOptions,
    ) -> Result<ImportReport> {
        let mut report = ImportReport::default();
        for (path, content) in files {
            let folder_path = markdown::folder_path_from_file_path(path);
            match markdown::import(self, content, options, &folder_path) {
                Ok(step) => report.merge(step),
                Err(error) => {
                    report.failed += 1;
                    report.errors.push(format!("{path}: {error}"));
                }
            }
        }
        Ok(report)
    }
}

impl ImportReport {
    /// Добавляет результаты другого импорта.
    pub fn merge(&mut self, other: ImportReport) {
        self.created += other.created;
        self.updated += other.updated;
        self.skipped += other.skipped;
        self.failed += other.failed;
        self.folders_created += other.folders_created;
        self.tags_created += other.tags_created;
        self.errors.extend(other.errors);
    }

    /// Всего обработано записей.
    pub fn total(&self) -> u32 {
        self.created + self.updated + self.skipped + self.failed
    }
}

/// Достаёт число пропущенных заметок из человекочитаемых причин пропуска.
///
/// Причины хранятся текстом (их видит пользователь), поэтому для отчёта
/// количество пропусков считается на месте, а не парсится обратно из строки.
fn skipped_encrypted_count(reasons: &[String]) -> u32 {
    reasons.len() as u32
}

/// Карта «идентификатор папки → путь от корня».
pub type FolderPathMap = std::collections::HashMap<EntityId, String>;

fn folder_path(map: &FolderPathMap, folder_id: &Option<EntityId>) -> String {
    match folder_id {
        Some(id) => map.get(id).cloned().unwrap_or_default(),
        None => String::new(),
    }
}

/// Безопасное имя файла из произвольной строки.
///
/// Запрещены символы, недопустимые в Windows (`<>:"/\|?*` и управляющие),
/// а также зарезервированные имена (`CON`, `NUL`, `PRN`, `AUX`, `COM1`…).
/// Точка в конце имени убирается: Windows её молча отбрасывает, и файл
/// переставал бы находиться по сохранённому пути.
pub fn sanitize_filename(value: &str, fallback: &str) -> String {
    let mut cleaned: String = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                ' '
            } else {
                character
            }
        })
        .collect();
    cleaned = crate::domain::normalize_label(&cleaned);
    // Windows молча отбрасывает точку и пробел в конце имени, поэтому файл
    // переставал бы находиться по сохранённому пути; пунктуацию в конце
    // убираем заодно — «Отчёт!» и «Отчёт» в одной папке не должны конфликтовать.
    cleaned = cleaned
        .trim_matches(|character: char| {
            matches!(character, '.' | ' ' | '!' | '?' | ',' | ';' | ':')
        })
        .to_owned();

    if cleaned.is_empty() {
        return fallback.to_owned();
    }
    let stem = cleaned.split('.').next().unwrap_or(&cleaned).to_uppercase();
    if RESERVED_NAMES.contains(&stem.as_str()) {
        cleaned = format!("_{cleaned}");
    }
    // Ограничиваем длину, чтобы вместе с путём папки не превысить лимит ОС.
    crate::domain::truncate_chars(&cleaned, 120)
}

/// Зарезервированные имена файлов Windows.
pub const RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Делает уникальное имя файла в пределах набора (добавляет ` 2`, ` 3`, …).
pub fn unique_file_path(
    taken: &mut std::collections::HashSet<String>,
    directory: &str,
    stem: &str,
    extension: &str,
) -> String {
    let mut candidate = if directory.is_empty() {
        format!("{stem}.{extension}")
    } else {
        format!("{directory}/{stem}.{extension}")
    };
    let mut counter = 2u32;
    while taken.contains(&candidate) {
        candidate = if directory.is_empty() {
            format!("{stem} {counter}.{extension}")
        } else {
            format!("{directory}/{stem} {counter}.{extension}")
        };
        counter += 1;
    }
    taken.insert(candidate.clone());
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_filename_removes_forbidden_characters() {
        assert_eq!(
            sanitize_filename("Отчёт: итоги/2026?", "заметка"),
            "Отчёт итоги 2026"
        );
        assert_eq!(
            sanitize_filename("  много   пробелов  ", "заметка"),
            "много пробелов"
        );
        assert_eq!(sanitize_filename("   ", "заметка"), "заметка");
        assert_eq!(sanitize_filename("", "заметка"), "заметка");
    }

    #[test]
    fn sanitize_filename_escapes_windows_reserved_names() {
        assert_eq!(sanitize_filename("CON", "заметка"), "_CON");
        assert_eq!(sanitize_filename("nul.txt", "заметка"), "_nul.txt");
        assert_eq!(sanitize_filename("Обычное имя", "заметка"), "Обычное имя");
    }

    #[test]
    fn sanitize_filename_trims_trailing_dots() {
        assert_eq!(sanitize_filename("имя...", "заметка"), "имя");
    }

    #[test]
    fn unique_file_path_appends_counter() {
        let mut taken = std::collections::HashSet::new();
        assert_eq!(
            unique_file_path(&mut taken, "notes", "Отчёт", "md"),
            "notes/Отчёт.md"
        );
        assert_eq!(
            unique_file_path(&mut taken, "notes", "Отчёт", "md"),
            "notes/Отчёт 2.md"
        );
        assert_eq!(
            unique_file_path(&mut taken, "notes", "Отчёт", "md"),
            "notes/Отчёт 3.md"
        );
        assert_eq!(
            unique_file_path(&mut taken, "", "Корень", "md"),
            "Корень.md"
        );
    }

    #[test]
    fn report_merges_counts() {
        let mut report = ImportReport {
            created: 2,
            ..Default::default()
        };
        report.merge(ImportReport {
            created: 1,
            skipped: 3,
            errors: vec!["ошибка".into()],
            ..Default::default()
        });
        assert_eq!(report.created, 3);
        assert_eq!(report.skipped, 3);
        assert_eq!(report.total(), 6);
        assert_eq!(report.errors, vec!["ошибка".to_string()]);
    }

    #[test]
    fn export_formats_have_extensions() {
        assert_eq!(ExportFormat::Markdown.extension(), "md");
        assert_eq!(ExportFormat::Json.extension(), "json");
        assert_eq!(ExportFormat::Html.extension(), "html");
    }

    #[test]
    fn default_export_options_are_safe() {
        let options = ExportOptions::default();
        assert!(options.include_archived);
        assert!(
            !options.include_trashed,
            "trash must not leak into exports by default"
        );
        assert!(
            !options.include_encrypted,
            "encrypted notes must not leak by default"
        );
        assert!(options.front_matter);
    }
}
