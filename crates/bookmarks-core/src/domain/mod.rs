//! Доменные модели и их инварианты.
//!
//! Модели одновременно являются контрактом IPC: они сериализуются в JSON без
//! дополнительных DTO. Нормализация (trim, схлопывание пробелов, ограничение
//! длины) выполняется здесь один раз, поэтому ни хранилище, ни фронтенд не
//! получают «грязные» данные.

pub mod folder;
pub mod limits;
pub mod note;
pub mod settings;
pub mod tag;

pub use folder::{Folder, FolderDraft, FolderPatch};
pub use note::{Note, NoteDraft, NotePatch, NoteSummary};
pub use settings::{
    AccentColor, BackupSettings, EditorSettings, EditorView, InterfaceSettings, Language,
    RemoteKind, Settings, SortOrder, SyncSettings, Theme, VaultSettings, VersionPolicy,
    VersionSettings,
};
pub use tag::{Tag, TagDraft};

/// Нормализует пользовательскую строку: убирает пробелы по краям, заменяет
/// переводы строк и табы пробелами, схлопывает повторяющиеся пробелы.
///
/// Применяется к названиям заметок, папок и тегов — там многострочность
/// бессмысленна, а лишние пробелы ломают сравнение и поиск.
pub fn normalize_label(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut previous_was_space = false;
    for character in value.chars() {
        if character.is_whitespace() {
            if !previous_was_space && !result.is_empty() {
                result.push(' ');
            }
            previous_was_space = true;
        } else {
            result.push(character);
            previous_was_space = false;
        }
    }
    // Пробел в конце не несёт смысла, но ломает сравнение имён папок и тегов.
    while result.ends_with(' ') {
        result.pop();
    }
    result
}

/// Обрезает строку до `max_chars` символов, не разрывая UTF-8 последовательности.
pub fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_label_collapses_whitespace() {
        assert_eq!(normalize_label("  hello   world \n\t "), "hello world");
        assert_eq!(normalize_label("\n\n"), "");
        assert_eq!(normalize_label("a"), "a");
        assert_eq!(normalize_label("Список   покупок"), "Список покупок");
    }

    #[test]
    fn truncate_chars_is_utf8_safe() {
        assert_eq!(truncate_chars("привет", 3), "при");
        assert_eq!(truncate_chars("привет", 10), "привет");
        assert_eq!(truncate_chars("", 5), "");
        // Эмодзи — один символ, но несколько байт: разрыва не происходит.
        assert_eq!(truncate_chars("🚀🚀🚀", 2), "🚀🚀");
    }
}
