//! Экспорт и импорт в Markdown.
//!
//! Файл экспорта = YAML front-matter (необязательный) + тело заметки. Такой
//! формат понимают Obsidian, Logseq, Joplin, Hugo и GitHub, поэтому выгрузка
//! не привязывает пользователя к Bookmarks.

use std::collections::HashSet;

use regex::Regex;

use crate::db::Store;
use crate::domain::Folder;
use crate::error::Result;
use crate::export::{
    sanitize_filename, unique_file_path, ExportNote, ExportOptions, ExportedFile,
    ImportConflictPolicy, ImportOptions, ImportReport,
};
use crate::time::Timestamp;

/// Имя служебного файла-оглавления.
pub const INDEX_FILE: &str = "index.md";

/// Разделитель front-matter.
const FRONT_MATTER_DELIMITER: &str = "---";

/// Ключ front-matter, по которому определяется наш собственный экспорт.
const FORMAT_MARKER: &str = "bookmarks";

/// Рендерит библиотеку в набор Markdown-файлов.
///
/// Структура каталогов повторяет дерево папок, поэтому выгрузку можно открыть
/// в любом редакторе и получить ту же иерархию. Оглавление `index.md` помогает
/// не потеряться после переноса.
pub fn render(
    notes: &[ExportNote],
    options: &ExportOptions,
    folders: &[Folder],
) -> Vec<ExportedFile> {
    let mut files = Vec::with_capacity(notes.len() + 1);
    let mut taken: HashSet<String> = HashSet::new();

    for note in notes {
        let directory = note
            .folder_path
            .split('/')
            .filter(|part| !part.is_empty())
            .map(|part| sanitize_filename(part, "папка"))
            .collect::<Vec<_>>()
            .join("/");
        let stem = if note.title.trim().is_empty() {
            format!("без названия {}", short_id(&note.id))
        } else {
            sanitize_filename(&note.title, "заметка")
        };
        let path = unique_file_path(&mut taken, &directory, &stem, "md");

        let mut content = String::new();
        if options.front_matter {
            content.push_str(&front_matter(note));
        }
        content.push_str(&note.body);
        if !content.ends_with('\n') {
            content.push('\n');
        }
        files.push(ExportedFile { path, content });
    }

    files.push(ExportedFile {
        path: INDEX_FILE.to_owned(),
        content: index_file(notes, folders),
    });
    files
}

/// YAML front-matter заметки.
pub fn front_matter(note: &ExportNote) -> String {
    let mut out = String::new();
    out.push_str(FRONT_MATTER_DELIMITER);
    out.push('\n');
    out.push_str(&format!("format: {}\n", FORMAT_MARKER));
    out.push_str(&format!("version: {}\n", crate::DATA_FORMAT_VERSION));
    out.push_str(&format!("id: {}\n", note.id));
    out.push_str(&format!("title: {}\n", yaml_string(&note.title)));
    out.push_str(&format!("created: {}\n", iso8601(note.created_at)));
    out.push_str(&format!("updated: {}\n", iso8601(note.updated_at)));
    if !note.folder_path.is_empty() {
        out.push_str(&format!("folder: {}\n", yaml_string(&note.folder_path)));
    }
    if !note.tags.is_empty() {
        out.push_str("tags:\n");
        for tag in &note.tags {
            out.push_str(&format!("  - {}\n", yaml_string(tag)));
        }
    }
    if note.pinned {
        out.push_str("pinned: true\n");
    }
    if note.archived {
        out.push_str("archived: true\n");
    }
    if let Some(color) = &note.color {
        out.push_str(&format!("color: {}\n", yaml_string(color)));
    }
    if note.encrypted {
        out.push_str("encrypted: true\n");
    }
    out.push_str(FRONT_MATTER_DELIMITER);
    out.push_str("\n\n");
    out
}

/// Экранирует строку для YAML: всегда используем двойные кавычки — это
/// единственный вариант, одинаково безопасный для кириллицы, двоеточий и `#`.
fn yaml_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            control if control.is_control() => escaped.push(' '),
            other => escaped.push(other),
        }
    }
    escaped.push('"');
    escaped
}

/// Метка времени в ISO 8601 (UTC) — человекочитаемо и переносимо.
fn iso8601(timestamp: Timestamp) -> String {
    match timestamp.to_datetime() {
        Some(datetime) => datetime.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        None => timestamp.millis().to_string(),
    }
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// Оглавление выгрузки.
fn index_file(notes: &[ExportNote], folders: &[Folder]) -> String {
    let mut out = String::new();
    out.push_str("# Bookmarks — выгрузка\n\n");
    out.push_str(&format!(
        "Заметок: {}. Папок: {}. Выгружено: {}.\n\n",
        notes.len(),
        folders.len(),
        iso8601(Timestamp::now())
    ));
    out.push_str("## Содержание\n\n");

    let mut by_folder: std::collections::BTreeMap<&str, Vec<&ExportNote>> =
        std::collections::BTreeMap::new();
    for note in notes {
        by_folder
            .entry(note.folder_path.as_str())
            .or_default()
            .push(note);
    }
    for (folder, group) in by_folder {
        let heading = if folder.is_empty() {
            "Без папки"
        } else {
            folder
        };
        out.push_str(&format!("### {heading}\n\n"));
        for note in group {
            let title = if note.title.trim().is_empty() {
                "Без названия"
            } else {
                &note.title
            };
            out.push_str(&format!("- {title}"));
            if note.encrypted {
                out.push_str(" 🔒");
            }
            if note.pinned {
                out.push_str(" 📌");
            }
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// Импорт
// ---------------------------------------------------------------------------

/// Разобранный Markdown-файл.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedMarkdown {
    /// Метаданные из front-matter.
    pub meta: MarkdownMeta,
    /// Тело без front-matter.
    pub body: String,
}

/// Метаданные из front-matter.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MarkdownMeta {
    /// Идентификатор заметки (если это наш экспорт).
    pub id: Option<String>,
    /// Заголовок.
    pub title: Option<String>,
    /// Путь папки.
    pub folder: Option<String>,
    /// Теги.
    pub tags: Vec<String>,
    /// Создана.
    pub created: Option<Timestamp>,
    /// Изменена.
    pub updated: Option<Timestamp>,
    /// Закреплена.
    pub pinned: bool,
    /// В архиве.
    pub archived: bool,
    /// Цвет.
    pub color: Option<String>,
    /// Это файл экспорта Bookmarks.
    pub ours: bool,
}

/// Разбирает файл: отделяет front-matter от тела.
///
/// Front-matter распознаётся только в самом начале файла и только если
/// закрывающий разделитель найден — иначе `---` внутри текста (горизонтальная
/// линейка) не должен «съедать» половину заметки.
pub fn parse(raw: &str) -> ParsedMarkdown {
    let trimmed = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    if !trimmed.starts_with(FRONT_MATTER_DELIMITER) {
        return ParsedMarkdown {
            meta: MarkdownMeta::default(),
            body: trimmed.trim_start_matches('\n').to_owned(),
        };
    }
    let after_first = &trimmed[FRONT_MATTER_DELIMITER.len()..];
    let Some((yaml_end, delimiter_end)) = find_closing_delimiter(after_first) else {
        return ParsedMarkdown {
            meta: MarkdownMeta::default(),
            body: trimmed.to_owned(),
        };
    };
    // Оба смещения — байтовые и указывают на границы строк, поэтому срезы
    // никогда не попадают внутрь многобайтового символа.
    let yaml = &after_first[..yaml_end];
    let body = after_first[delimiter_end..]
        .trim_start_matches(['\r', '\n'])
        .to_owned();
    ParsedMarkdown {
        meta: parse_front_matter(yaml),
        body,
    }
}

/// Ищет закрывающий `---`; возвращает (конец YAML, конец разделителя) в байтах.
fn find_closing_delimiter(text: &str) -> Option<(usize, usize)> {
    let mut offset = 0usize;
    for line in text.split_inclusive('\n') {
        let without_newline = line.trim_end_matches(['\n', '\r']);
        // Первая строка после открывающего разделителя не считается: `---`
        // в начале YAML был бы бессмысленным пустым блоком.
        if offset > 0 && without_newline.trim() == FRONT_MATTER_DELIMITER {
            let end = offset + without_newline.trim_start().len();
            return Some((offset, end));
        }
        offset += line.len();
    }
    None
}

/// Разбирает YAML front-matter.
///
/// Собственный YAML-парсер не используется: front-matter нашего экспорта
/// плоский (строки, булевы значения и один список тегов), а полноценная
/// библиотека добавила бы зависимость ради десятка строк. Неизвестные ключи
/// игнорируются, поэтому файлы Obsidian и Hugo импортируются без ошибок.
pub fn parse_front_matter(yaml: &str) -> MarkdownMeta {
    let mut meta = MarkdownMeta::default();
    let mut in_tags = false;

    for line in yaml.lines() {
        // Отступы в YAML обязательны, поэтому строка приводится к обоим видам:
        // `trimmed` — для списка (элементы всегда с отступом), `trimmed_end` —
        // для пар «ключ: значение».
        let trimmed_end = line.trim_end();
        let trimmed = trimmed_end.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("- ") || trimmed == "-" {
            if in_tags {
                let value = unquote(trimmed.trim_start_matches('-').trim());
                if !value.is_empty() {
                    meta.tags.push(value);
                }
            }
            continue;
        }
        in_tags = false;

        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        match key.as_str() {
            "format" => meta.ours = value == FORMAT_MARKER,
            "id" => meta.id = Some(unquote(value)).filter(|id| !id.is_empty()),
            "title" => meta.title = Some(unquote(value)).filter(|title| !title.is_empty()),
            "folder" | "path" | "directory" => {
                meta.folder = Some(unquote(value)).filter(|folder| !folder.is_empty())
            }
            "created" | "created_at" | "date" => meta.created = parse_timestamp(value),
            "updated" | "updated_at" | "modified" => meta.updated = parse_timestamp(value),
            "pinned" => meta.pinned = parse_bool(value),
            "archived" => meta.archived = parse_bool(value),
            "color" | "colour" => {
                meta.color = Some(unquote(value)).filter(|color| !color.is_empty())
            }
            "tags" | "tag" => {
                if value.is_empty() {
                    in_tags = true;
                } else {
                    // `tags: [a, b]` и `tags: a, b` — обе формы встречаются в дикой природе.
                    let inline =
                        value.trim_matches(|character| character == '[' || character == ']');
                    meta.tags.extend(
                        inline
                            .split(',')
                            .map(|part| unquote(part.trim()))
                            .filter(|part| !part.is_empty()),
                    );
                }
            }
            _ => {}
        }
    }
    meta
}

fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    let stripped = trimmed
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        })
        .unwrap_or(trimmed);
    stripped
        .replace("\\n", "\n")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}

fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "yes" | "1" | "on"
    )
}

fn parse_timestamp(value: &str) -> Option<Timestamp> {
    let cleaned = unquote(value);
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return None;
    }
    // Наш собственный формат: 2026-01-31T12:34:56.789Z
    if let Ok(datetime) = chrono::DateTime::parse_from_rfc3339(cleaned) {
        return Some(Timestamp::from_millis(datetime.timestamp_millis()));
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.3fZ",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d",
        "%d.%m.%Y",
    ] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(cleaned, format) {
            return Some(Timestamp::from_millis(naive.and_utc().timestamp_millis()));
        }
        if let Ok(date) = chrono::NaiveDate::parse_from_str(cleaned, format) {
            return Some(Timestamp::from_millis(
                date.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis(),
            ));
        }
    }
    // Голые миллисекунды — на случай выгрузки из другого приложения.
    cleaned.parse::<i64>().ok().map(Timestamp::from_millis)
}

/// Извлекает `#теги` из текста заметки.
///
/// Решётка заголовка (`## Раздел`) тегом не считается: перед `#` требуется
/// начало строки или пробел, а после — буква или цифра.
pub fn extract_inline_tags(body: &str) -> Vec<String> {
    let re = Regex::new(r"(?:^|[\s(,;:])#([^\s#!.,;:()\[\]{}<>]{1,60})").expect("static regex");
    let mut tags: Vec<String> = Vec::new();
    for captures in re.captures_iter(body) {
        if let Some(tag) = captures.get(1) {
            let name = crate::domain::tag::normalize_tag_name(tag.as_str());
            if name.is_empty() || name.chars().all(|character| character.is_ascii_digit()) {
                continue;
            }
            let lower = name.to_lowercase();
            if !tags.iter().any(|existing| existing.to_lowercase() == lower) {
                tags.push(name);
            }
        }
    }
    tags
}

/// Путь папки из пути файла (`Работа/Проекты/заметка.md` → `Работа/Проекты`).
pub fn folder_path_from_file_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let without_file = normalized
        .rsplit_once('/')
        .map(|(directory, _)| directory)
        .unwrap_or("");
    without_file
        .split('/')
        .filter(|part| !part.is_empty() && *part != INDEX_FILE)
        .collect::<Vec<_>>()
        .join("/")
}

/// Импортирует один Markdown-файл.
pub fn import(
    store: &Store,
    raw: &str,
    options: &ImportOptions,
    folder_path: &str,
) -> Result<ImportReport> {
    let parsed = parse(raw);
    let mut report = ImportReport::default();

    let body = parsed.body.trim();
    if body.is_empty() && parsed.meta.title.is_none() {
        report.skipped += 1;
        report.errors.push("пустой файл пропущен".to_owned());
        return Ok(report);
    }

    let mut tags = parsed.meta.tags.clone();
    if options.extract_inline_tags {
        for tag in extract_inline_tags(body) {
            if !tags
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&tag))
            {
                tags.push(tag);
            }
        }
    }

    let title = parsed
        .meta
        .title
        .clone()
        .or_else(|| crate::domain::note::title_from_body(body))
        .unwrap_or_default();

    let effective_folder = if options.create_folders {
        parsed
            .meta
            .folder
            .clone()
            .unwrap_or_else(|| folder_path.to_owned())
    } else {
        String::new()
    };

    let folder_id = if effective_folder.trim().is_empty() {
        options.folder_id.clone()
    } else {
        match store.ensure_folder_path(&effective_folder) {
            Ok(id) => {
                report.folders_created += 1;
                Some(id)
            }
            Err(error) => {
                report.errors.push(format!(
                    "не удалось создать папку «{effective_folder}»: {error}"
                ));
                options.folder_id.clone()
            }
        }
    };

    let content_hash = crate::domain::note::content_hash(&title, body);
    if let Some(existing_id) = store.find_note_by_hash(&content_hash, parsed.meta.id.as_deref())? {
        match options.conflict_policy {
            ImportConflictPolicy::Skip => {
                report.skipped += 1;
                return Ok(report);
            }
            ImportConflictPolicy::Overwrite => {
                store.update_note(
                    &existing_id,
                    crate::domain::note::NotePatch {
                        title: Some(title.clone()),
                        body: Some(body.to_owned()),
                        folder_id: Some(folder_id.clone()),
                        tags: Some(tags.clone()),
                        pinned: Some(parsed.meta.pinned),
                        archived: Some(parsed.meta.archived),
                        color: Some(parsed.meta.color.clone()),
                    },
                )?;
                report.updated += 1;
                return Ok(report);
            }
            ImportConflictPolicy::Duplicate => {
                // Падаем в создание новой заметки ниже.
            }
        }
    }

    store.create_note(crate::domain::note::NoteDraft {
        title: Some(title),
        body: Some(body.to_owned()),
        folder_id,
        tags,
        pinned: Some(parsed.meta.pinned),
        color: parsed.meta.color.clone(),
        encrypted: Some(false),
    })?;
    report.created += 1;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::ExportNote;

    fn note(title: &str, body: &str, folder: &str) -> ExportNote {
        ExportNote {
            id: crate::ids::new_id(),
            title: title.to_owned(),
            body: body.to_owned(),
            folder_id: None,
            folder_path: folder.to_owned(),
            tags: vec!["работа".into()],
            pinned: false,
            archived: false,
            color: Some("blue".into()),
            encrypted: false,
            created_at: Timestamp::from_millis(1_700_000_000_000),
            updated_at: Timestamp::from_millis(1_700_000_500_000),
            content_hash: String::new(),
        }
    }

    #[test]
    fn renders_front_matter_and_body() {
        let files = render(
            &[note("Отчёт", "# Отчёт\n\nТекст", "Работа")],
            &ExportOptions::default(),
            &[],
        );
        assert_eq!(files.len(), 2, "note + index");
        let content = &files[0].content;
        assert!(content.starts_with("---\nformat: bookmarks\n"), "{content}");
        assert!(content.contains("title: \"Отчёт\""));
        assert!(content.contains("tags:\n  - \"работа\""));
        assert!(content.contains("color: \"blue\""));
        assert!(content.contains("# Отчёт"));
        assert_eq!(files[0].path, "Работа/Отчёт.md");
        assert_eq!(files[1].path, INDEX_FILE);
        assert!(files[1].content.contains("Отчёт"));
    }

    #[test]
    fn skips_front_matter_when_disabled() {
        let options = ExportOptions {
            front_matter: false,
            ..Default::default()
        };
        let files = render(&[note("Заметка", "текст", "")], &options, &[]);
        assert!(!files[0].content.contains("format: bookmarks"));
        assert!(files[0].content.starts_with("текст"));
        assert_eq!(files[0].path, "Заметка.md");
    }

    #[test]
    fn sanitizes_paths_and_avoids_collisions() {
        let first = note("Отчёт: итоги?", "a", "Работа/Проект «А»");
        let second = note("Отчёт: итоги!", "b", "Работа/Проект «А»");
        let files = render(&[first, second], &ExportOptions::default(), &[]);
        assert_eq!(files[0].path, "Работа/Проект «А»/Отчёт итоги.md");
        assert_eq!(files[1].path, "Работа/Проект «А»/Отчёт итоги 2.md");
    }

    #[test]
    fn parses_own_front_matter_roundtrip() {
        let original = note(
            "Заголовок с: двоеточием",
            "Тело\n\n- пункт",
            "Папка/Подпапка",
        );
        let files = render(
            std::slice::from_ref(&original),
            &ExportOptions::default(),
            &[],
        );
        let parsed = parse(&files[0].content);
        assert!(parsed.meta.ours);
        assert_eq!(
            parsed.meta.title.as_deref(),
            Some("Заголовок с: двоеточием")
        );
        assert_eq!(parsed.meta.folder.as_deref(), Some("Папка/Подпапка"));
        assert_eq!(parsed.meta.tags, vec!["работа".to_string()]);
        assert_eq!(parsed.meta.color.as_deref(), Some("blue"));
        assert_eq!(
            parsed.meta.created.map(|value| value.millis()),
            Some(1_700_000_000_000)
        );
        assert_eq!(
            parsed.meta.updated.map(|value| value.millis()),
            Some(1_700_000_500_000)
        );
        assert_eq!(parsed.meta.id.as_deref(), Some(original.id.as_str()));
        assert!(parsed.body.starts_with("Тело"));
    }

    #[test]
    fn parses_foreign_front_matter() {
        let raw =
            "---\ntitle: Obsidian note\ntags: [idea, todo]\ndate: 2026-01-31\n---\n\nСодержимое";
        let parsed = parse(raw);
        assert!(!parsed.meta.ours);
        assert_eq!(parsed.meta.title.as_deref(), Some("Obsidian note"));
        assert_eq!(
            parsed.meta.tags,
            vec!["idea".to_string(), "todo".to_string()]
        );
        assert!(parsed.meta.created.is_some());
        assert_eq!(parsed.body, "Содержимое");
    }

    #[test]
    fn horizontal_rule_is_not_front_matter() {
        let raw = "Первая строка\n\n---\n\nВторая строка";
        let parsed = parse(raw);
        assert!(!parsed.meta.ours);
        assert!(parsed.body.contains("Первая строка"));
        assert!(parsed.body.contains("---"));
    }

    #[test]
    fn unterminated_front_matter_stays_body() {
        let raw = "---\ntitle: Сломанный\n\nТело без закрывающего разделителя";
        let parsed = parse(raw);
        assert!(parsed.meta.title.is_none());
        assert!(parsed.body.contains("Сломанный"));
    }

    #[test]
    fn handles_bom_and_crlf() {
        let raw = "\u{feff}---\r\ntitle: Windows\r\n---\r\n\r\nТело";
        let parsed = parse(raw);
        assert_eq!(parsed.meta.title.as_deref(), Some("Windows"));
        assert!(parsed.body.starts_with("Тело"));
    }

    #[test]
    fn extracts_inline_tags_without_headings() {
        let body = "## Раздел\n\nКупить #молоко и #хлеб.\nСсылка https://x.com/#anchor не тег.\n#123 тоже не тег.\n#Молоко — дубликат.";
        let tags = extract_inline_tags(body);
        assert!(tags.iter().any(|tag| tag == "молоко"));
        assert!(tags.iter().any(|tag| tag == "хлеб"));
        assert!(!tags.iter().any(|tag| tag.contains("anchor")));
        assert!(!tags.iter().any(|tag| tag == "123"));
        assert_eq!(
            tags.iter()
                .filter(|tag| tag.to_lowercase() == "молоко")
                .count(),
            1
        );
    }

    #[test]
    fn folder_path_from_file_path_works() {
        assert_eq!(
            folder_path_from_file_path("Работа/Проекты/заметка.md"),
            "Работа/Проекты"
        );
        assert_eq!(folder_path_from_file_path("заметка.md"), "");
        assert_eq!(
            folder_path_from_file_path("C:\\Users\\x\\notes\\a.md"),
            "C:/Users/x/notes"
        );
    }

    #[test]
    fn timestamp_parsing_covers_common_formats() {
        assert!(parse_timestamp("2026-01-31T12:34:56.789Z").is_some());
        assert!(parse_timestamp("2026-01-31T12:34:56Z").is_some());
        assert!(parse_timestamp("2026-01-31").is_some());
        assert!(parse_timestamp("31.01.2026").is_some());
        assert!(parse_timestamp("1700000000000").is_some());
        assert!(parse_timestamp("не дата").is_none());
        assert!(parse_timestamp("").is_none());
    }

    #[test]
    fn yaml_strings_are_safe() {
        assert_eq!(yaml_string("обычный"), "\"обычный\"");
        assert_eq!(yaml_string("с \"кавычками\""), "\"с \\\"кавычками\\\"\"");
        assert_eq!(
            yaml_string("многострочный\nтекст"),
            "\"многострочный\\nтекст\""
        );
        assert_eq!(yaml_string("ключ: значение"), "\"ключ: значение\"");
    }
}
