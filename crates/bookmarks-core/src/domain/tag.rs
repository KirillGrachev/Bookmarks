//! Теги — плоская разметка поверх дерева папок.

use crate::domain::limits;
use crate::error::{Error, Result};
use crate::ids::EntityId;
use crate::time::Timestamp;

/// Тег.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    /// Публичный идентификатор.
    pub id: EntityId,
    /// Название (уникальное без учёта регистра).
    pub name: String,
    /// Цвет из палитры.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Создан.
    pub created_at: Timestamp,
    /// Число заметок с тегом (вычисляемое поле).
    #[serde(default)]
    pub note_count: u32,
}

/// Данные для создания тега.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagDraft {
    /// Название.
    pub name: String,
    /// Цвет.
    pub color: Option<String>,
}

impl TagDraft {
    /// Проверяет черновик тега.
    pub fn validate(&self) -> Result<()> {
        validate_name(&self.name)?;
        match self.color.as_deref() {
            Some(value) if !limits::is_allowed_color(value) => {
                Err(Error::invalid_field("недопустимый цвет тега", "color"))
            }
            _ => Ok(()),
        }
    }
}

/// Нормализует название тега: убирает ведущий `#`, пробелы и схлопывает их.
///
/// Пользователи вводят и `#работа`, и `работа` — это один и тот же тег.
/// Сравнение в хранилище идёт по нижнему регистру, поэтому `Работа` и
/// `работа` тоже не размножаются.
pub fn normalize_tag_name(value: &str) -> String {
    crate::domain::normalize_label(value.trim().trim_start_matches('#'))
}

fn validate_name(name: &str) -> Result<()> {
    let normalized = normalize_tag_name(name);
    if normalized.is_empty() {
        return Err(Error::invalid_field(
            "название тега не может быть пустым",
            "name",
        ));
    }
    if normalized.chars().count() > limits::MAX_TAG_NAME_CHARS {
        return Err(Error::invalid_field(
            format!(
                "название тега длиннее {} символов",
                limits::MAX_TAG_NAME_CHARS
            ),
            "name",
        ));
    }
    if normalized.chars().any(|character| character.is_control()) {
        return Err(Error::invalid_field(
            "название тега содержит управляющие символы",
            "name",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_hash_and_whitespace() {
        assert_eq!(normalize_tag_name("#работа"), "работа");
        assert_eq!(normalize_tag_name("  ##идеи  "), "идеи");
        assert_eq!(normalize_tag_name("важное  дело"), "важное дело");
        assert_eq!(normalize_tag_name("#"), "");
    }

    #[test]
    fn validates_names_and_colors() {
        assert!(matches!(
            TagDraft {
                name: "#".into(),
                ..Default::default()
            }
            .validate(),
            Err(Error::InvalidInput { .. })
        ));
        assert!(matches!(
            TagDraft {
                name: "т".repeat(limits::MAX_TAG_NAME_CHARS + 1),
                ..Default::default()
            }
            .validate(),
            Err(Error::InvalidInput { .. })
        ));
        assert!(matches!(
            TagDraft {
                name: "ок".into(),
                color: Some("silver".into()),
            }
            .validate(),
            Err(Error::InvalidInput { .. })
        ));
        assert!(TagDraft {
            name: "#работа".into(),
            color: Some("blue".into()),
        }
        .validate()
        .is_ok());
    }
}
