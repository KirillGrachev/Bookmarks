//! Заметка — главная сущность приложения.

use crate::domain::limits;
use crate::error::{Error, Result};
use crate::ids::EntityId;
use crate::time::Timestamp;

/// Тело заметки в Markdown.
///
/// Хранится как есть: редактор работает с исходным текстом, а рендер
/// выполняется на фронтенде. Для полнотекстового поиска из Markdown
/// дополнительно извлекается обычный текст (см. `search::plaintext`).
pub type MarkdownBody = String;

/// Заметка в том виде, в каком её знает приложение.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// Публичный идентификатор (UUID v4).
    pub id: EntityId,
    /// Заголовок. Пустая строка означает «Без названия» — это показывает UI.
    pub title: String,
    /// Тело заметки в Markdown.
    pub body: MarkdownBody,
    /// Родительская папка; `None` — заметка лежит в «Черновиках» (корень).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<EntityId>,
    /// Названия тегов заметки, отсортированные по алфавиту.
    ///
    /// Именно названия, а не идентификаторы: они одинаковы на всех устройствах
    /// (в отличие от внутренних ключей), поэтому их можно показывать в
    /// интерфейсе, экспортировать и синхронизировать без дополнительных
    /// запросов.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Закреплена в начале списка.
    pub pinned: bool,
    /// В архиве: не участвует в обычной выдаче и в поиске по умолчанию.
    pub archived: bool,
    /// Цветовая метка (имя из палитры) или `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Момент создания.
    pub created_at: Timestamp,
    /// Момент последнего изменения содержимого или метаданных.
    pub updated_at: Timestamp,
    /// Момент помещения в корзину; `None` — заметка не удалена.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<Timestamp>,
    /// Хеш содержимого (blake3 от `title + body`) — для детектора изменений в синхронизации.
    pub content_hash: String,
    /// Число слов в теле (для статусной строки редактора).
    pub word_count: u32,
    /// Тело зашифровано в сейфе: `body` приходит с фронтенда уже пустым,
    /// а расшифровка происходит только после разблокировки.
    pub encrypted: bool,
}

/// Данные для создания заметки.
///
/// `id`, метки времени и хеш вычисляются на стороне ядра — клиент не может
/// подделать ни порядок создания, ни контрольную сумму.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteDraft {
    /// Заголовок (необязательный).
    pub title: Option<String>,
    /// Тело в Markdown (необязательное).
    pub body: Option<String>,
    /// Родительская папка.
    pub folder_id: Option<EntityId>,
    /// Названия тегов — существующие переиспользуются, новые создаются.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Закрепить сразу.
    pub pinned: Option<bool>,
    /// Цветовая метка.
    pub color: Option<String>,
    /// Поместить в сейф (требует разблокированного хранилища).
    pub encrypted: Option<bool>,
}

/// Частичное обновление заметки.
///
/// `None` означает «не трогать», `Some(None)` — «сбросить» (актуально для
/// папки и цвета). Благодаря этому автосейв отправляет только изменённые поля.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePatch {
    /// Новый заголовок.
    pub title: Option<String>,
    /// Новое тело.
    pub body: Option<String>,
    /// Новая папка (`Some(None)` — переместить в корень).
    pub folder_id: Option<Option<EntityId>>,
    /// Новый набор тегов (замена целиком).
    pub tags: Option<Vec<String>>,
    /// Закрепление.
    pub pinned: Option<bool>,
    /// Архив.
    pub archived: Option<bool>,
    /// Цветовая метка (`Some(None)` — снять).
    pub color: Option<Option<String>>,
}

/// Облегчённая заметка для виртуализированного списка.
///
/// Список показывает тысячи строк, а тело заметки ему не нужно: отдельный
/// «лёгкий» запрос уменьшает объём IPC в десятки раз.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummary {
    /// Идентификатор.
    pub id: EntityId,
    /// Заголовок.
    pub title: String,
    /// Фрагмент тела без разметки (первые ~180 символов) — для превью в списке.
    pub excerpt: String,
    /// Родительская папка.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<EntityId>,
    /// Названия тегов (уже разрезовленные, чтобы список не делал второй запрос).
    #[serde(default)]
    pub tags: Vec<String>,
    /// Закреплена.
    pub pinned: bool,
    /// В архиве.
    pub archived: bool,
    /// Цветовая метка.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Зашифрована.
    pub encrypted: bool,
    /// Создана.
    pub created_at: Timestamp,
    /// Изменена.
    pub updated_at: Timestamp,
}

impl Note {
    /// Проверяет и нормализует данные черновика.
    ///
    /// Ошибки возвращаются с указанием поля, чтобы форма могла их подсветить.
    pub fn validate_draft(draft: &NoteDraft) -> Result<()> {
        if let Some(title) = &draft.title {
            validate_title(title)?;
        }
        if let Some(body) = &draft.body {
            validate_body(body)?;
        }
        validate_color(draft.color.as_deref())?;
        Ok(())
    }

    /// Проверяет и нормализует патч.
    pub fn validate_patch(patch: &NotePatch) -> Result<()> {
        if let Some(title) = &patch.title {
            validate_title(title)?;
        }
        if let Some(body) = &patch.body {
            validate_body(body)?;
        }
        if let Some(color) = patch.color.as_ref().and_then(Option::as_deref) {
            validate_color(Some(color))?;
        }
        if let Some(tags) = &patch.tags {
            if tags.len() > limits::MAX_TAGS_PER_NOTE {
                return Err(Error::invalid_field(
                    format!("не более {} тегов у заметки", limits::MAX_TAGS_PER_NOTE),
                    "tags",
                ));
            }
        }
        Ok(())
    }

    /// Пересчитывает производные поля: хеш содержимого и число слов.
    pub fn refresh_derived(&mut self) {
        self.content_hash = content_hash(&self.title, &self.body);
        self.word_count = count_words(&self.body);
    }
}

/// Хеш содержимого заметки: blake3 от заголовка и тела.
///
/// Используется синхронизацией, чтобы не передавать и не сравнивать мегабайты
/// текста, и историей версий — чтобы не сохранять снапшот без изменений.
pub fn content_hash(title: &str, body: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"bookmarks/note/v1\0");
    hasher.update(title.as_bytes());
    hasher.update(b"\0");
    hasher.update(body.as_bytes());
    hasher.finalize().to_hex().to_string()
}

/// Число слов в тексте.
///
/// Считаем по тексту без разметки: `**один**` — это одно слово «один», а не
/// токены из звёздочек. Совпадает с тем, что видит пользователь в режиме
/// чтения и что попадает в поисковый индекс.
pub fn count_words(body: &str) -> u32 {
    let text = if body.contains(['*', '#', '[', ']', '`', '>', '|']) {
        crate::search::plaintext(body)
    } else {
        body.to_owned()
    };
    text.split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|token| !token.is_empty())
        .count() as u32
}

/// Извлекает заголовок из первой строки Markdown, если явной заголовок пуст.
///
/// Поведение «как в Obsidian/Bear»: `# Мой заголовок` в теле становится
/// названием заметки в списке, а в редакторе остаётся текстом.
pub fn title_from_body(body: &str) -> Option<String> {
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let without_markers = trimmed.trim_start_matches('#').trim();
        if without_markers.is_empty() {
            continue;
        }
        return Some(crate::domain::truncate_chars(
            without_markers,
            limits::MAX_TITLE_CHARS,
        ));
    }
    None
}

fn validate_title(title: &str) -> Result<()> {
    if title.chars().count() > limits::MAX_TITLE_CHARS {
        return Err(Error::invalid_field(
            format!("заголовок длиннее {} символов", limits::MAX_TITLE_CHARS),
            "title",
        ));
    }
    // Управляющие символы в заголовке ломают вёрстку и CSV/JSON-экспорт.
    if title.chars().any(char::is_control) {
        return Err(Error::invalid_field(
            "заголовок содержит управляющие символы",
            "title",
        ));
    }
    Ok(())
}

fn validate_body(body: &str) -> Result<()> {
    if body.chars().count() > limits::MAX_BODY_CHARS {
        return Err(Error::invalid_field(
            format!("заметка больше {} символов", limits::MAX_BODY_CHARS),
            "body",
        ));
    }
    Ok(())
}

fn validate_color(color: Option<&str>) -> Result<()> {
    match color {
        Some(value) if !limits::is_allowed_color(value) => {
            Err(Error::invalid_field("недопустимый цвет метки", "color"))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> NoteDraft {
        NoteDraft {
            title: Some("  Список покупок  ".into()),
            body: Some("# Список\n- молоко\n- хлеб".into()),
            ..Default::default()
        }
    }

    #[test]
    fn validates_acceptable_draft() {
        assert!(Note::validate_draft(&draft()).is_ok());
    }

    #[test]
    fn rejects_oversized_title_with_field() {
        let draft = NoteDraft {
            title: Some("а".repeat(limits::MAX_TITLE_CHARS + 1)),
            ..Default::default()
        };
        let error = Note::validate_draft(&draft).expect_err("too long");
        match error {
            Error::InvalidInput { field, .. } => assert_eq!(field.as_deref(), Some("title")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn rejects_control_characters_in_title() {
        let draft = NoteDraft {
            title: Some("плохой\u{0}заголовок".into()),
            ..Default::default()
        };
        assert!(matches!(
            Note::validate_draft(&draft),
            Err(Error::InvalidInput { .. })
        ));
    }

    #[test]
    fn rejects_unknown_color() {
        assert!(matches!(
            Note::validate_draft(&NoteDraft {
                color: Some("chartreuse".into()),
                ..Default::default()
            }),
            Err(Error::InvalidInput { .. })
        ));
        assert!(Note::validate_draft(&NoteDraft {
            color: Some("blue".into()),
            ..Default::default()
        })
        .is_ok());
        assert!(Note::validate_draft(&NoteDraft {
            color: None,
            ..Default::default()
        })
        .is_ok());
    }

    #[test]
    fn rejects_oversized_body() {
        let draft = NoteDraft {
            body: Some("x".repeat(limits::MAX_BODY_CHARS + 1)),
            ..Default::default()
        };
        assert!(matches!(
            Note::validate_draft(&draft),
            Err(Error::InvalidInput { .. })
        ));
    }

    #[test]
    fn patch_color_can_be_cleared_but_not_invented() {
        assert!(Note::validate_patch(&NotePatch {
            color: Some(None),
            ..Default::default()
        })
        .is_ok());
        assert!(matches!(
            Note::validate_patch(&NotePatch {
                color: Some(Some("neon".into())),
                ..Default::default()
            }),
            Err(Error::InvalidInput { .. })
        ));
    }

    #[test]
    fn content_hash_is_stable_and_sensitive() {
        let first = content_hash("Заголовок", "Тело");
        assert_eq!(first, content_hash("Заголовок", "Тело"));
        assert_ne!(first, content_hash("Заголовок", "Тело "));
        assert_ne!(first, content_hash("Заголовок2", "Тело"));
        assert_eq!(first.len(), 64);
    }

    #[test]
    fn counts_words_ignoring_markup() {
        assert_eq!(count_words(""), 0);
        assert_eq!(count_words("раз два три"), 3);
        assert_eq!(count_words("# Заголовок\n\n- пункт **один**"), 3);
        assert_eq!(count_words("hello_world 42"), 2);
    }

    #[test]
    fn derives_title_from_first_heading() {
        assert_eq!(
            title_from_body("# Мой заголовок\nтекст").as_deref(),
            Some("Мой заголовок")
        );
        assert_eq!(
            title_from_body("\n\n## Второй\n").as_deref(),
            Some("Второй")
        );
        assert_eq!(
            title_from_body("просто текст").as_deref(),
            Some("просто текст")
        );
        assert_eq!(title_from_body("   "), None);
        assert_eq!(title_from_body("###"), None);
    }

    #[test]
    fn refresh_derived_updates_hash_and_words() {
        let mut note = Note {
            id: crate::ids::new_id(),
            title: "Старый".into(),
            body: "одно два".into(),
            folder_id: None,
            tags: vec![],
            pinned: false,
            archived: false,
            color: None,
            created_at: Timestamp::now(),
            updated_at: Timestamp::now(),
            deleted_at: None,
            content_hash: String::new(),
            word_count: 0,
            encrypted: false,
        };
        note.refresh_derived();
        assert_eq!(note.word_count, 2);
        assert_eq!(note.content_hash, content_hash("Старый", "одно два"));
        note.body = "одно два три".into();
        note.refresh_derived();
        assert_eq!(note.word_count, 3);
        assert_ne!(note.content_hash, content_hash("Старый", "одно два"));
    }
}
