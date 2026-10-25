//! Примитивы хранилища для синхронизации.
//!
//! Здесь только атомарные операции «прочитать слепок», «применить удалённую
//! запись», «снять версию перед перезаписью». Логика принятия решений (что
//! считать конфликтом и кто победил) живёт в `sync::engine` — такое разделение
//! позволяет тестировать слияние на хранилище в памяти, не имитируя сеть.

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::write::read_vault_record;
use crate::db::Store;
use crate::domain::note::{content_hash, count_words};
use crate::error::{Error, Result};
use crate::ids::EntityId;
use crate::time::Timestamp;
use crate::versions::{self, VersionOrigin};

/// Заметка в формате обмена синхронизации.
///
/// В отличие от `domain::Note`, здесь теги заданы названиями (а не
/// идентификаторами): на другом устройстве у того же тега может быть другой
/// внутренний ключ, а название — общая правда.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncNote {
    /// Идентификатор заметки.
    pub id: EntityId,
    /// Заголовок.
    pub title: String,
    /// Тело (или контейнер сейфа, если `encrypted`).
    pub body: String,
    /// Идентификатор папки.
    pub folder_id: Option<EntityId>,
    /// Путь папки от корня через `/`.
    ///
    /// Идентификаторы на разных устройствах не совпадают, поэтому структура
    /// папок переносится именно путём: принимающая сторона воссоздаёт её и
    /// подставляет свой локальный идентификатор.
    #[serde(default)]
    pub folder_path: String,
    /// Названия тегов.
    pub tags: Vec<String>,
    /// Закреплена.
    pub pinned: bool,
    /// В архиве.
    pub archived: bool,
    /// Цветовая метка.
    pub color: Option<String>,
    /// Зашифрована.
    pub encrypted: bool,
    /// Создана.
    pub created_at: Timestamp,
    /// Изменена.
    pub updated_at: Timestamp,
    /// Удалена (надгробие — другие устройства должны удалить её у себя).
    pub deleted: bool,
    /// Хеш содержимого.
    pub content_hash: String,
}

impl Store {
    /// Слепок всех заметок, включая удалённые (нужны для надгробий).
    pub fn sync_snapshot(&self) -> Result<Vec<SyncNote>> {
        let pooled = self.connection()?;
        let connection = pooled.get();
        let mut statement = connection.prepare(
            "SELECT n.uid, n.title, n.body, f.uid, n.pinned, n.archived, n.color, n.encrypted,
                    n.created_at, n.updated_at, n.deleted_at, n.content_hash
             FROM notes n LEFT JOIN folders f ON f.id = n.folder_id
             ORDER BY n.uid",
        )?;
        let rows = statement.query_map([], |row| {
            let deleted_at: Option<i64> = row.get(10)?;
            Ok(SyncNote {
                id: row.get(0)?,
                title: row.get(1)?,
                body: row.get(2)?,
                folder_id: row.get(3)?,
                folder_path: String::new(),
                tags: Vec::new(),
                pinned: row.get::<_, i64>(4)? != 0,
                archived: row.get::<_, i64>(5)? != 0,
                color: row.get(6)?,
                encrypted: row.get::<_, i64>(7)? != 0,
                created_at: Timestamp::from_millis(row.get(8)?),
                updated_at: Timestamp::from_millis(row.get(9)?),
                deleted: deleted_at.is_some(),
                content_hash: row.get(11)?,
            })
        })?;
        let mut notes = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        let folder_paths = self.folder_path_map()?;
        for note in &mut notes {
            note.folder_path = note
                .folder_id
                .as_ref()
                .and_then(|id| folder_paths.get(id))
                .cloned()
                .unwrap_or_default();
        }
        for note in &mut notes {
            note.tags = tag_names(connection, &note.id)?;
        }
        Ok(notes)
    }

    /// Одна заметка в формате обмена.
    pub fn get_sync_note(&self, id: &str) -> Result<Option<SyncNote>> {
        let pooled = self.connection()?;
        let connection = pooled.get();
        let row: Option<SyncNote> = connection
            .query_row(
                "SELECT n.uid, n.title, n.body, f.uid, n.pinned, n.archived, n.color, n.encrypted,
                        n.created_at, n.updated_at, n.deleted_at, n.content_hash
                 FROM notes n LEFT JOIN folders f ON f.id = n.folder_id WHERE n.uid = ?1",
                params![id],
                |row| {
                    let deleted_at: Option<i64> = row.get(10)?;
                    Ok(SyncNote {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        body: row.get(2)?,
                        folder_id: row.get(3)?,
                        folder_path: String::new(),
                        tags: Vec::new(),
                        pinned: row.get::<_, i64>(4)? != 0,
                        archived: row.get::<_, i64>(5)? != 0,
                        color: row.get(6)?,
                        encrypted: row.get::<_, i64>(7)? != 0,
                        created_at: Timestamp::from_millis(row.get(8)?),
                        updated_at: Timestamp::from_millis(row.get(9)?),
                        deleted: deleted_at.is_some(),
                        content_hash: row.get(11)?,
                    })
                },
            )
            .optional()?;
        let Some(mut note) = row else {
            return Ok(None);
        };
        note.tags = tag_names(connection, id)?;
        if let Some(folder_id) = &note.folder_id {
            note.folder_path = self
                .folder_path_map()?
                .remove(folder_id)
                .unwrap_or_default();
        }
        Ok(Some(note))
    }

    /// Применяет запись, пришедшую из удалённого хранилища.
    ///
    /// Заметка создаётся, если её ещё нет, и обновляется иначе. Папка и теги
    /// разрешаются по идентификаторам и названиям; если папки с таким
    /// идентификатором на этом устройстве нет, заметка попадает в корень —
    /// потеря места хранения предпочтительнее отказа принять правку.
    pub fn apply_remote_note(&self, note: &SyncNote) -> Result<()> {
        // Путь папки разрешается до транзакции: создание папок само по себе
        // транзакционно, а вложенное заимствование соединения нам не нужно.
        let resolved_folder = match (&note.folder_id, note.folder_path.trim()) {
            (_, path) if !path.is_empty() => Some(self.ensure_folder_path(path)?),
            (Some(uid), _) => uid.clone().into(),
            (None, _) => None,
        };
        let resolved_folder = match resolved_folder {
            Some(id) => Some(id),
            None => note.folder_id.clone(),
        };

        self.transaction(|connection| {
            let folder_key = match &resolved_folder {
                Some(uid) => folder_key_by_uid(connection, uid)?,
                None => None,
            };
            let existing: Option<(i64, String, String)> = connection
                .query_row(
                    "SELECT id, title, body FROM notes WHERE uid = ?1",
                    params![note.id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?;

            let deleted_at = note.deleted.then(|| Timestamp::now().millis());
            let hash = if note.content_hash.is_empty() {
                content_hash(&note.title, &note.body)
            } else {
                note.content_hash.clone()
            };

            match existing {
                Some((key, previous_title, previous_body)) => {
                    if previous_title != note.title || previous_body != note.body {
                        versions::save_version(
                            connection,
                            key,
                            &previous_title,
                            &previous_body,
                            VersionOrigin::Sync,
                        )?;
                    }
                    connection.execute(
                        "UPDATE notes SET title = ?2, body = ?3, folder_id = ?4, pinned = ?5,
                                          archived = ?6, color = ?7, encrypted = ?8, created_at = ?9,
                                          updated_at = ?10, deleted_at = ?11, content_hash = ?12,
                                          word_count = ?13
                         WHERE id = ?1",
                        params![
                            key,
                            note.title,
                            note.body,
                            folder_key,
                            i64::from(note.pinned),
                            i64::from(note.archived),
                            note.color,
                            i64::from(note.encrypted),
                            note.created_at.millis(),
                            note.updated_at.millis(),
                            deleted_at,
                            hash,
                            i64::from(count_words(&note.body)),
                        ],
                    )?;
                    replace_tags(connection, key, &note.tags)?;
                }
                None => {
                    if !crate::ids::is_valid_id(&note.id) {
                        return Err(Error::sync(
                            format!("принятый идентификатор заметки некорректен: {}", note.id),
                            crate::error::SyncErrorCode::RemoteFormat,
                        ));
                    }
                    connection.execute(
                        "INSERT INTO notes (uid, title, body, folder_id, pinned, archived, color,
                                            encrypted, created_at, updated_at, deleted_at,
                                            content_hash, word_count)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                        params![
                            note.id,
                            note.title,
                            note.body,
                            folder_key,
                            i64::from(note.pinned),
                            i64::from(note.archived),
                            note.color,
                            i64::from(note.encrypted),
                            note.created_at.millis(),
                            note.updated_at.millis(),
                            deleted_at,
                            hash,
                            i64::from(count_words(&note.body)),
                        ],
                    )?;
                    let key = connection.query_row(
                        "SELECT id FROM notes WHERE uid = ?1",
                        params![note.id],
                        |row| row.get::<_, i64>(0),
                    )?;
                    replace_tags(connection, key, &note.tags)?;
                }
            }

            // Поисковый индекс обновляют триггеры: они же исключают тело
            // зашифрованной заметки из выдачи.
            Ok(())
        })
    }

    /// Сохраняет версию перед перезаписью чужими данными.
    pub fn save_remote_version(&self, note_id: &str, title: &str, body: &str) -> Result<bool> {
        self.transaction(|connection| {
            let key = note_key_by_uid(connection, note_id)?;
            versions::save_version(connection, key, title, body, VersionOrigin::Sync)
        })
    }

    /// Создаёт заметку-копию для неразрешённого конфликта.
    ///
    /// Копия получает суффикс в заголовке и тег `конфликт`, чтобы пользователь
    /// нашёл все такие места одним запросом (`tag:конфликт`) и разобрал их.
    pub fn create_conflict_copy(&self, note: &SyncNote, device: &str) -> Result<EntityId> {
        let suffix = if device.trim().is_empty() {
            "конфликт".to_owned()
        } else {
            format!("конфликт с {device}")
        };
        let title = crate::domain::truncate_chars(
            &format!("{} ({suffix})", note.title),
            crate::domain::limits::MAX_TITLE_CHARS,
        );
        let mut tags = note.tags.clone();
        if !tags.iter().any(|tag| tag.eq_ignore_ascii_case("конфликт")) {
            tags.push("конфликт".to_owned());
        }
        let copy = SyncNote {
            id: crate::ids::new_id(),
            title,
            body: note.body.clone(),
            folder_id: note.folder_id.clone(),
            folder_path: note.folder_path.clone(),
            tags,
            pinned: false,
            archived: note.archived,
            color: note.color.clone(),
            encrypted: false,
            created_at: Timestamp::now(),
            updated_at: Timestamp::now(),
            deleted: false,
            content_hash: String::new(),
        };
        self.apply_remote_note(&copy)?;
        Ok(copy.id)
    }

    /// Синхронизированы ли настройки сейфа с принятыми данными.
    ///
    /// Если из хранилища пришла зашифрованная заметка, а сейф на этом
    /// устройстве не инициализирован, прочитать её нельзя — приложение должно
    /// показать понятную подсказку, а не пустой редактор.
    pub fn vault_required_for_synced_notes(&self) -> Result<bool> {
        let pooled = self.connection()?;
        let connection = pooled.get();
        let has_encrypted: i64 = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM notes WHERE encrypted = 1)",
            [],
            |row| row.get(0),
        )?;
        if has_encrypted == 0 {
            return Ok(false);
        }
        Ok(read_vault_record(connection)?.is_none())
    }
}

impl Store {
    /// Карта «идентификатор папки → путь от корня через `/`».
    pub fn folder_path_map(&self) -> Result<crate::export::FolderPathMap> {
        let mut map = crate::export::FolderPathMap::new();
        fn walk(nodes: &[crate::db::read::FolderNode], map: &mut crate::export::FolderPathMap) {
            for node in nodes {
                map.insert(node.folder.id.clone(), node.path.clone());
                walk(&node.children, map);
            }
        }
        walk(&self.folder_tree()?, &mut map);
        Ok(map)
    }

    /// Находит существующую заметку по идентификатору или по хешу содержимого.
    ///
    /// Используется импортом: идентификатор срабатывает при переносе нашей
    /// собственной копии, хеш — при импорте из чужого формата, где
    /// идентификаторов нет вовсе.
    pub fn find_note_by_hash(
        &self,
        hash: &str,
        uid_hint: Option<&str>,
    ) -> Result<Option<EntityId>> {
        let pooled = self.connection()?;
        let connection = pooled.get();
        if let Some(uid) = uid_hint {
            let found: Option<EntityId> = connection
                .query_row(
                    "SELECT uid FROM notes WHERE uid = ?1",
                    params![uid],
                    |row| row.get(0),
                )
                .optional()?;
            if found.is_some() {
                return Ok(found);
            }
        }
        if hash.is_empty() {
            return Ok(None);
        }
        connection
            .query_row(
                "SELECT uid FROM notes WHERE content_hash = ?1 AND deleted_at IS NULL ORDER BY updated_at DESC LIMIT 1",
                params![hash],
                |row| row.get(0),
            )
            .optional()
            .map_err(Error::from)
    }

    /// Находит или создаёт папку по имени внутри указанного родителя.
    ///
    /// Возвращает идентификатор папки. Ошибка — только при нарушении
    /// доменных ограничений (пустое имя, недопустимый цвет).
    pub fn ensure_folder(
        &self,
        name: &str,
        parent_id: Option<&str>,
        color: Option<&str>,
    ) -> Result<EntityId> {
        let normalized = crate::domain::truncate_chars(
            &crate::domain::normalize_label(name),
            crate::domain::limits::MAX_FOLDER_NAME_CHARS,
        );
        if normalized.is_empty() {
            return Err(Error::invalid_field(
                "название папки не может быть пустым",
                "name",
            ));
        }
        if let Some(value) = color {
            if !crate::domain::limits::is_allowed_color(value) {
                return Err(Error::invalid_field("недопустимый цвет папки", "color"));
            }
        }

        let existing = self.list_folders()?.into_iter().find(|folder| {
            folder.name.eq_ignore_ascii_case(&normalized)
                && folder.parent_id.as_deref() == parent_id
        });
        if let Some(folder) = existing {
            return Ok(folder.id);
        }
        let created = self.create_folder(crate::domain::FolderDraft {
            name: normalized,
            parent_id: parent_id.map(str::to_owned),
            color: color.map(str::to_owned),
            icon: None,
        })?;
        Ok(created.id)
    }

    /// Создаёт цепочку папок по пути «Работа/Проекты/2026».
    pub fn ensure_folder_path(&self, path: &str) -> Result<EntityId> {
        let parts: Vec<&str> = path
            .split('/')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect();
        if parts.is_empty() {
            return Err(Error::invalid("пустой путь папки"));
        }
        let mut parent: Option<EntityId> = None;
        let mut last = String::new();
        for part in parts {
            last = self.ensure_folder(part, parent.as_deref(), None)?;
            parent = Some(last.clone());
        }
        Ok(last)
    }
}

fn note_key_by_uid(connection: &Connection, uid: &str) -> Result<i64> {
    connection
        .query_row("SELECT id FROM notes WHERE uid = ?1", params![uid], |row| {
            row.get(0)
        })
        .optional()?
        .ok_or_else(|| Error::not_found("note", uid))
}

fn folder_key_by_uid(connection: &Connection, uid: &str) -> Result<Option<i64>> {
    connection
        .query_row(
            "SELECT id FROM folders WHERE uid = ?1",
            params![uid],
            |row| row.get(0),
        )
        .optional()
        .map_err(Error::from)
}

fn tag_names(connection: &Connection, note_uid: &str) -> Result<Vec<String>> {
    let mut statement = connection.prepare(
        "SELECT t.name FROM note_tags nt JOIN tags t ON t.id = nt.tag_id
         JOIN notes n ON n.id = nt.note_id WHERE n.uid = ?1 ORDER BY lower(t.name)",
    )?;
    let rows = statement.query_map(params![note_uid], |row| row.get::<_, String>(0))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::from)
}

/// Заменяет набор тегов заметки, создавая отсутствующие.
fn replace_tags(connection: &Connection, note_key: i64, names: &[String]) -> Result<()> {
    connection.execute(
        "DELETE FROM note_tags WHERE note_id = ?1",
        params![note_key],
    )?;
    let now = Timestamp::now();
    for raw in names {
        let name = crate::domain::truncate_chars(
            &crate::domain::tag::normalize_tag_name(raw),
            crate::domain::limits::MAX_TAG_NAME_CHARS,
        );
        if name.is_empty() {
            continue;
        }
        let lower = name.to_lowercase();
        let existing: Option<i64> = connection
            .query_row(
                "SELECT id FROM tags WHERE name_lower = ?1",
                params![lower],
                |row| row.get(0),
            )
            .optional()?;
        let tag_key = match existing {
            Some(key) => key,
            None => {
                connection.execute(
                    "INSERT INTO tags (uid, name, name_lower, color, created_at) VALUES (?1, ?2, ?3, NULL, ?4)",
                    params![crate::ids::new_id(), name, lower, now.millis()],
                )?;
                connection.query_row("SELECT last_insert_rowid()", [], |row| row.get(0))?
            }
        };
        connection.execute(
            "INSERT OR IGNORE INTO note_tags (note_id, tag_id) VALUES (?1, ?2)",
            params![note_key, tag_key],
        )?;
    }
    Ok(())
}
