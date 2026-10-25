//! Полнотекстовый поиск: извлечение текста из Markdown, парсер запроса,
//! построение MATCH-выражения для FTS5.
//!
//! Синтаксис запроса (как в крупных заметочниках, но без surprises):
//! - обычные слова ищутся по префиксу: `закуп` находит «закупки»;
//! - `"точная фраза"` — поиск словосочетания;
//! - `tag:работа`, `#работа` — фильтр по тегу;
//! - `folder:Проекты` — фильтр по названию папки;
//! - `is:pinned`, `is:archived`, `is:encrypted`, `is:empty`, `is:attachment` — флаги;
//! - `has:image`, `has:link`, `has:code`, `has:task` — по содержимому Markdown;
//! - `color:red` — цветовая метка;
//! - `after:2026-01-01`, `before:2026-02-01` — диапазон дат изменения;
//! - `-слово` — исключение.
//!
//! Операторы `tag:`/`folder:`/`is:`/`has:`/`color:`/`before:`/`after:`
//! намеренно не отправляются в FTS5: они применяются к результатам как
//! постфильтр, поэтому работают одинаково стабильно на любой локали и не
//! ломают ранжирование.

pub mod plaintext;
pub mod query;

pub use plaintext::plaintext;
pub use query::{FilterSpec, QueryToken, SearchQuery};

/// Применяет не-текстовые операторы запроса к краткому описанию заметки.
///
/// Вынесено отдельно от SQL, чтобы один и тот же парсер обслуживал и поиск по
/// базе, и мгновенную фильтрацию уже загруженного списка на фронтенде
/// (логика продублирована в `src/lib/searchQuery.ts` и покрыта тестами с обеих
/// сторон — расхождение поведения поймает CI).
pub fn matches_summary(
    summary: &crate::domain::note::NoteSummary,
    filters: &[FilterSpec],
    tag_names: &[String],
    folder_path: Option<&str>,
) -> bool {
    filters.iter().all(|filter| match filter {
        FilterSpec::Tag { name, negated } => {
            let present = tag_names.iter().any(|tag| tag.eq_ignore_ascii_case(name));
            present != *negated
        }
        FilterSpec::Folder { name, negated } => {
            let present = folder_path
                .map(|path| {
                    path.split(" / ")
                        .any(|part| part.eq_ignore_ascii_case(name))
                })
                .unwrap_or(false);
            present != *negated
        }
        FilterSpec::Flag { flag, negated } => {
            let present = match flag.as_str() {
                "pinned" => summary.pinned,
                "archived" => summary.archived,
                "encrypted" => summary.encrypted,
                "empty" => summary.excerpt.trim().is_empty() && summary.title.trim().is_empty(),
                _ => false,
            };
            present != *negated
        }
        FilterSpec::Color { name, negated } => {
            let present = summary.color.as_deref() == Some(name.as_str());
            present != *negated
        }
        // Содержимое Markdown и даты проверяются там, где есть полный текст
        // заметки (см. `matches_note`), а не по краткому описанию.
        FilterSpec::Has { .. }
        | FilterSpec::UpdatedAfter { .. }
        | FilterSpec::UpdatedBefore { .. } => true,
    })
}

/// Применяет операторы `has:` к тексту заметки.
pub fn matches_body(body: &str, filters: &[FilterSpec]) -> bool {
    filters.iter().all(|filter| match filter {
        FilterSpec::Has { kind, negated } => {
            let present = match kind.as_str() {
                "image" => plaintext::contains_image(body),
                "link" => plaintext::contains_link(body),
                "code" => plaintext::contains_code(body),
                "task" => plaintext::contains_task(body),
                "table" => plaintext::contains_table(body),
                "attachment" => plaintext::contains_attachment(body),
                _ => false,
            };
            present != *negated
        }
        _ => true,
    })
}

/// Применяет не-текстовые операторы к полной заметке.
pub fn matches_note(
    note: &crate::domain::note::Note,
    filters: &[FilterSpec],
    tag_names: &[String],
) -> bool {
    filters.iter().all(|filter| match filter {
        FilterSpec::Has { kind, negated } => {
            let present = match kind.as_str() {
                "image" => plaintext::contains_image(&note.body),
                "link" => plaintext::contains_link(&note.body),
                "code" => plaintext::contains_code(&note.body),
                "task" => plaintext::contains_task(&note.body),
                "table" => plaintext::contains_table(&note.body),
                "attachment" => plaintext::contains_attachment(&note.body),
                _ => false,
            };
            present != *negated
        }
        FilterSpec::UpdatedAfter { millis } => note.updated_at.millis() >= *millis,
        FilterSpec::UpdatedBefore { millis } => note.updated_at.millis() <= *millis,
        FilterSpec::Tag { name, negated } => {
            let present = tag_names.iter().any(|tag| tag.eq_ignore_ascii_case(name));
            present != *negated
        }
        FilterSpec::Flag { flag, negated } => {
            let present = match flag.as_str() {
                "pinned" => note.pinned,
                "archived" => note.archived,
                "encrypted" => note.encrypted,
                "empty" => note.title.trim().is_empty() && note.body.trim().is_empty(),
                _ => false,
            };
            present != *negated
        }
        FilterSpec::Color { name, negated } => {
            let present = note.color.as_deref() == Some(name.as_str());
            present != *negated
        }
        FilterSpec::Folder { .. } => true,
    })
}
