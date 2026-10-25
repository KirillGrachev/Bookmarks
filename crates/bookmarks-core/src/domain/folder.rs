//! Папки — дерево размещения заметок.

use crate::domain::limits;
use crate::error::{Error, Result};
use crate::ids::EntityId;
use crate::time::Timestamp;

/// Папка (может быть вложенной).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    /// Публичный идентификатор.
    pub id: EntityId,
    /// Название.
    pub name: String,
    /// Родительская папка; `None` — корень дерева.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<EntityId>,
    /// Цветовая метка из палитры.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Иконка (имя из набора lucide, например `book-open`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Порядок среди соседей.
    pub sort_order: i32,
    /// Создана.
    pub created_at: Timestamp,
    /// Изменена.
    pub updated_at: Timestamp,
    /// Число заметок прямо в папке (вычисляемое поле для списка).
    #[serde(default)]
    pub note_count: u32,
}

/// Данные для создания папки.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderDraft {
    /// Название (обязательное).
    pub name: String,
    /// Родительская папка.
    pub parent_id: Option<EntityId>,
    /// Цвет.
    pub color: Option<String>,
    /// Иконка.
    pub icon: Option<String>,
}

/// Частичное обновление папки.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderPatch {
    /// Новое название.
    pub name: Option<String>,
    /// Новый родитель (`Some(None)` — поднять в корень).
    pub parent_id: Option<Option<EntityId>>,
    /// Новый цвет (`Some(None)` — снять).
    pub color: Option<Option<String>>,
    /// Новая иконка (`Some(None)` — снять).
    pub icon: Option<Option<String>>,
    /// Новая позиция среди соседей.
    pub sort_order: Option<i32>,
}

impl FolderDraft {
    /// Проверяет черновик папки.
    pub fn validate(&self) -> Result<()> {
        validate_name(&self.name)?;
        validate_color(self.color.as_deref())?;
        Ok(())
    }
}

impl FolderPatch {
    /// Проверяет патч папки.
    pub fn validate(&self) -> Result<()> {
        if let Some(name) = &self.name {
            validate_name(name)?;
        }
        if let Some(color) = self.color.as_ref().and_then(Option::as_deref) {
            validate_color(Some(color))?;
        }
        Ok(())
    }

    /// Новый родитель, если он задан в патче.
    pub fn parent(&self) -> Option<Option<EntityId>> {
        self.parent_id.clone()
    }
}

fn validate_name(name: &str) -> Result<()> {
    let normalized = crate::domain::normalize_label(name);
    if normalized.is_empty() {
        return Err(Error::invalid_field(
            "название папки не может быть пустым",
            "name",
        ));
    }
    if normalized.chars().count() > limits::MAX_FOLDER_NAME_CHARS {
        return Err(Error::invalid_field(
            format!(
                "название длиннее {} символов",
                limits::MAX_FOLDER_NAME_CHARS
            ),
            "name",
        ));
    }
    Ok(())
}

fn validate_color(color: Option<&str>) -> Result<()> {
    match color {
        Some(value) if !limits::is_allowed_color(value) => {
            Err(Error::invalid_field("недопустимый цвет папки", "color"))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_long_names() {
        assert!(matches!(
            FolderDraft {
                name: "   ".into(),
                ..Default::default()
            }
            .validate(),
            Err(Error::InvalidInput { .. })
        ));
        assert!(matches!(
            FolderDraft {
                name: "п".repeat(limits::MAX_FOLDER_NAME_CHARS + 1),
                ..Default::default()
            }
            .validate(),
            Err(Error::InvalidInput { .. })
        ));
        assert!(FolderDraft {
            name: "Работа".into(),
            ..Default::default()
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn validates_color_on_create_and_update() {
        assert!(matches!(
            FolderDraft {
                name: "x".into(),
                color: Some("lime".into()),
                ..Default::default()
            }
            .validate(),
            Err(Error::InvalidInput { .. })
        ));
        assert!(FolderPatch {
            color: Some(Some("violet".into())),
            ..Default::default()
        }
        .validate()
        .is_ok());
        assert!(FolderPatch {
            color: Some(None),
            ..Default::default()
        }
        .validate()
        .is_ok());
    }
}
