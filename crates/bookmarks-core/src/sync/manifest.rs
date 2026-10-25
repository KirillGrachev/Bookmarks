//! Манифест устройства — компактный слепок состояния библиотеки.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::ids::EntityId;
use crate::time::Timestamp;

/// Магический префикс файла манифеста.
pub const MANIFEST_MAGIC: &str = "BKMAN1";

/// Текущая версия формата манифеста.
pub const MANIFEST_VERSION: u32 = 1;

/// Отпечаток заметки в манифесте.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestNote {
    /// Хеш содержимого (`title + body`) — главный признак изменения.
    pub content_hash: String,
    /// Отпечаток состояния: содержимое И признак удаления.
    ///
    /// Сравнивать только `content_hash` нельзя: помещение заметки в корзину не
    /// меняет её текст, и удаление не доехало бы до других устройств.
    #[serde(default)]
    pub fingerprint: String,
    /// Метка последнего изменения.
    pub updated_at: Timestamp,
    /// Заметка удалена (мягкое удаление синхронизируется как удаление).
    #[serde(default)]
    pub deleted: bool,
    /// Заметка зашифрована в сейфе: тело выгружается контейнером.
    #[serde(default)]
    pub encrypted: bool,
    /// Родительская папка.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<EntityId>,
}

/// Манифест одного устройства.
///
/// `BTreeMap` вместо `HashMap` намеренно: порядок ключей стабилен, поэтому
/// файл одинакового состояния байт-в-байт совпадает между запусками и не
/// создаёт ложных изменений при синхронизации и в системе контроля версий.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceManifest {
    /// Маркер формата.
    pub magic: String,
    /// Версия формата.
    pub version: u32,
    /// Идентификатор устройства.
    pub device_id: EntityId,
    /// Человекочитаемое имя устройства (показывается в списке конфликтов).
    #[serde(default)]
    pub device_name: String,
    /// Момент формирования манифеста.
    pub generated_at: Timestamp,
    /// Отпечатки заметок по их идентификаторам.
    pub notes: BTreeMap<EntityId, ManifestNote>,
    /// Отпечатки папок: идентификатор → (хеш полей, метка изменения).
    #[serde(default)]
    pub folders: BTreeMap<EntityId, FolderFingerprint>,
    /// Отпечатки тегов.
    #[serde(default)]
    pub tags: BTreeMap<EntityId, FolderFingerprint>,
}

/// Отпечаток папки или тега.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderFingerprint {
    /// Хеш значимых полей.
    pub content_hash: String,
    /// Метка изменения.
    pub updated_at: Timestamp,
    /// Удалена.
    #[serde(default)]
    pub deleted: bool,
}

/// Вычисляет отпечаток: хеш содержимого плюс признак удаления.
pub fn fingerprint(content_hash: &str, deleted: bool) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"bookmarks/manifest-note/v1\0");
    hasher.update(content_hash.as_bytes());
    hasher.update(if deleted { b"\0deleted" } else { b"\0alive" });
    hasher.finalize().to_hex().to_string()
}

impl DeviceManifest {
    /// Создаёт пустой манифест для устройства.
    pub fn new(device_id: EntityId, device_name: String) -> Self {
        Self {
            magic: MANIFEST_MAGIC.to_owned(),
            version: MANIFEST_VERSION,
            device_id,
            device_name,
            generated_at: Timestamp::now(),
            notes: BTreeMap::new(),
            folders: BTreeMap::new(),
            tags: BTreeMap::new(),
        }
    }

    /// Сериализует в форматированный JSON.
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Разбирает JSON, проверяя маркер и версию формата.
    ///
    /// Чужой или более новый файл отвергается целиком: частично принятый
    /// манифест привёл бы к ложным удалениям заметок на других устройствах.
    pub fn from_json(raw: &str) -> Result<Self> {
        let manifest: Self = serde_json::from_str(raw)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Проверяет маркер формата и совместимость версии.
    pub fn validate(&self) -> Result<()> {
        if self.magic != MANIFEST_MAGIC {
            return Err(Error::sync(
                format!(
                    "файл не является манифестом Bookmarks (magic = {})",
                    self.magic
                ),
                crate::error::SyncErrorCode::RemoteFormat,
            ));
        }
        if self.version > MANIFEST_VERSION {
            return Err(Error::sync(
                format!(
                    "манифест версии {} новее поддерживаемой ({})",
                    self.version, MANIFEST_VERSION
                ),
                crate::error::SyncErrorCode::RemoteFormat,
            ));
        }
        if self.device_id.trim().is_empty() {
            return Err(Error::sync(
                "в манифесте нет идентификатора устройства",
                crate::error::SyncErrorCode::RemoteFormat,
            ));
        }
        Ok(())
    }

    /// Число неотменённых заметок.
    pub fn active_note_count(&self) -> usize {
        self.notes.values().filter(|note| !note.deleted).count()
    }

    /// Отпечаток состояния заметки для сравнения между устройствами.
    ///
    /// Для манифестов старого формата (поле отсутствует) вычисляется из имеющихся
    /// данных, поэтому старая копия не считается «изменившейся целиком».
    pub fn fingerprint_of(note: &ManifestNote) -> String {
        if note.fingerprint.is_empty() {
            fingerprint(note.content_hash.as_str(), note.deleted)
        } else {
            note.fingerprint.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> DeviceManifest {
        let mut manifest = DeviceManifest::new("device-1".into(), "Рабочий ноутбук".into());
        manifest.notes.insert(
            "note-1".into(),
            ManifestNote {
                content_hash: "hash-1".into(),
                fingerprint: fingerprint("hash-1", false),
                updated_at: Timestamp::from_millis(1_700_000_000_000),
                deleted: false,
                encrypted: false,
                folder_id: None,
            },
        );
        manifest
    }

    #[test]
    fn roundtrips_through_json() {
        let original = manifest();
        let parsed = DeviceManifest::from_json(&original.to_json().expect("json")).expect("parse");
        assert_eq!(parsed, original);
        assert_eq!(parsed.active_note_count(), 1);
    }

    #[test]
    fn serialization_is_stable() {
        let first = manifest();
        let mut second = manifest();
        second.notes.insert(
            "note-0".into(),
            ManifestNote {
                content_hash: "hash-0".into(),
                fingerprint: fingerprint("hash-0", false),
                updated_at: Timestamp::from_millis(1_600_000_000_000),
                deleted: false,
                encrypted: false,
                folder_id: None,
            },
        );
        // Ключи сортируются, поэтому порядок вставки не влияет на байты.
        let json_a = first.to_json().expect("json");
        let json_b = second.to_json().expect("json");
        assert!(json_b.contains("note-0"));
        assert_eq!(json_a, first.to_json().expect("json"));
    }

    #[test]
    fn rejects_foreign_magic() {
        let mut foreign = manifest();
        foreign.magic = "KMPBENC1".into();
        let error =
            DeviceManifest::from_json(&foreign.to_json().expect("json")).expect_err("must fail");
        assert!(matches!(error, Error::Sync { .. }));
    }

    #[test]
    fn rejects_newer_version() {
        let mut future = manifest();
        future.version = MANIFEST_VERSION + 1;
        let error =
            DeviceManifest::from_json(&future.to_json().expect("json")).expect_err("must fail");
        assert!(error.to_string().contains("новее поддерживаемой"));
    }

    #[test]
    fn rejects_missing_device_id() {
        let mut broken = manifest();
        broken.device_id = "  ".into();
        assert!(broken.validate().is_err());
    }

    #[test]
    fn rejects_garbage_json() {
        assert!(DeviceManifest::from_json("{").is_err());
        assert!(DeviceManifest::from_json("").is_err());
    }
}
