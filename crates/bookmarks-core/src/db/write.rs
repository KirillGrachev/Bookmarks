//! Запись: создание, обновление, удаление заметок, папок и тегов.
//!
//! Все составные операции выполняются в транзакции: заметка и её теги, версия
//! и поисковый индекс, перемещение папки и её поддерево должны меняться
//! атомарно. Перед изменением содержимого автоматически сохраняется версия —
//! история работает без явных действий пользователя.

use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension};

use crate::crypto::{self, VaultRecord, VaultSession};
use crate::db::read::NOTE_COLUMNS;
use crate::db::Store;
use crate::domain::limits;
use crate::domain::note::{content_hash, count_words, Note, NoteDraft, NotePatch};
use crate::domain::tag::normalize_tag_name;
use crate::domain::{normalize_label, truncate_chars, Folder, FolderDraft, FolderPatch, TagDraft};
use crate::error::{Error, Result};
use crate::ids::{new_id, EntityId};
use crate::time::Timestamp;
use crate::versions::VersionOrigin;

impl Store {
    /// Создаёт заметку.
    ///
    /// Если заголовок не задан, он берётся из первой строки Markdown — так
    /// заметка сразу получает осмысленное имя в списке.
    pub fn create_note(&self, draft: NoteDraft) -> Result<Note> {
        Note::validate_draft(&draft)?;
        let now = Timestamp::now();
        let body = draft.body.unwrap_or_default();
        let title = match draft.title {
            Some(title) => truncate_chars(&normalize_label(&title), limits::MAX_TITLE_CHARS),
            None => crate::domain::note::title_from_body(&body).unwrap_or_default(),
        };
        let vault = Arc::clone(&self.vault);

        self.transaction(|connection| {
            let uid = new_id();
            let folder_key = match &draft.folder_id {
                Some(id) => Some(
                    folder_key_by_uid(connection, id)?
                        .ok_or_else(|| Error::not_found("folder", id.clone()))?,
                ),
                None => None,
            };
            let record = read_vault_record(connection)?;
            let encrypted = vault_mode(&record, draft.encrypted)?;
            let stored_body = match (&record, encrypted) {
                (Some(record), true) => {
                    crypto::encrypt_container(&unlocked_key(&vault, record)?, body.as_bytes())?
                }
                _ => body.clone(),
            };

            connection.execute(
                "INSERT INTO notes (uid, title, body, folder_id, pinned, archived, color, encrypted,
                                    created_at, updated_at, deleted_at, content_hash, word_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8, ?8, NULL, ?9, ?10)",
                params![
                    uid,
                    title,
                    stored_body,
                    folder_key,
                    i64::from(draft.pinned.unwrap_or(false)),
                    draft.color.clone(),
                    i64::from(encrypted),
                    now.millis(),
                    content_hash(&title, &body),
                    i64::from(count_words(&body)),
                ],
            )?;

            let note_key = note_key_by_uid(connection, &uid)?;
            let tag_ids = resolve_tags(connection, &draft.tags)?;
            link_tags(connection, note_key, &tag_ids)?;

            let mut note = read_note(connection, &uid)?;
            note.tags = tag_names_of(connection, note_key)?;
            Ok(note)
        })
    }

    /// Обновляет заметку; при изменении содержимого сохраняет версию.
    pub fn update_note(&self, id: &str, patch: NotePatch) -> Result<Note> {
        Note::validate_patch(&patch)?;
        let now = Timestamp::now();
        let vault = Arc::clone(&self.vault);

        self.transaction(|connection| {
            let existing = read_note(connection, id)?;
            let note_key = note_key_by_uid(connection, id)?;
            let record = read_vault_record(connection)?;

            let next_title = patch
                .title
                .map(|title| truncate_chars(&normalize_label(&title), limits::MAX_TITLE_CHARS))
                .unwrap_or(existing.title.clone());
            let next_body = patch.body.clone().unwrap_or(existing.body.clone());

            // Для зашифрованных заметок в базе лежит контейнер, поэтому
            // предыдущее состояние доступно только при разблокированном сейфе.
            let previous_plain = match (&record, existing.encrypted) {
                (Some(record), true) => Some(crypto::decrypt_container(
                    &unlocked_key(&vault, record)?,
                    &existing.body,
                )?),
                _ => Some(existing.body.clone()),
            };

            // Версия снимается с ПРЕДЫДУЩЕГО состояния: откат должен
            // возвращать текст, который пользователь видел до правки.
            let content_changed = next_title != existing.title
                || previous_plain.as_deref() != Some(next_body.as_str());
            if content_changed {
                if let Some(plain) = previous_plain.as_deref() {
                    crate::versions::save_version(
                        connection,
                        note_key,
                        &existing.title,
                        plain,
                        VersionOrigin::Edit,
                    )?;
                }
            }

            let folder_key = match patch.folder_id.clone() {
                Some(Some(folder_id)) => Some(
                    folder_key_by_uid(connection, &folder_id)?
                        .ok_or_else(|| Error::not_found("folder", folder_id))?,
                ),
                Some(None) => None,
                None => existing_folder_key(connection, note_key),
            };
            let next_color = match patch.color.clone() {
                Some(color) => color,
                None => existing.color.clone(),
            };
            let next_pinned = patch.pinned.unwrap_or(existing.pinned);
            let next_archived = patch.archived.unwrap_or(existing.archived);
            let encrypted = vault_mode(&record, Some(existing.encrypted))?;
            let stored_body = match (&record, encrypted) {
                (Some(record), true) => {
                    crypto::encrypt_container(&unlocked_key(&vault, record)?, next_body.as_bytes())?
                }
                _ => next_body.clone(),
            };

            connection.execute(
                "UPDATE notes SET title = ?2, body = ?3, folder_id = ?4, pinned = ?5, archived = ?6,
                                   color = ?7, encrypted = ?8, updated_at = ?9, content_hash = ?10,
                                   word_count = ?11
                 WHERE id = ?1",
                params![
                    note_key,
                    next_title,
                    stored_body,
                    folder_key,
                    i64::from(next_pinned),
                    i64::from(next_archived),
                    next_color,
                    i64::from(encrypted),
                    now.millis(),
                    content_hash(&next_title, &next_body),
                    i64::from(count_words(&next_body)),
                ],
            )?;

            if let Some(tag_names) = patch.tags.clone() {
                let tag_ids = resolve_tags(connection, &tag_names)?;
                connection.execute(
                    "DELETE FROM note_tags WHERE note_id = ?1",
                    params![note_key],
                )?;
                link_tags(connection, note_key, &tag_ids)?;
            }

            let mut note = read_note(connection, id)?;
            note.tags = tag_names_of(connection, note_key)?;
            Ok(note)
        })
    }

    /// Закрепляет или открепляет заметку.
    pub fn set_note_pinned(&self, id: &str, pinned: bool) -> Result<Note> {
        self.update_note(
            id,
            NotePatch {
                pinned: Some(pinned),
                ..Default::default()
            },
        )
    }

    /// Помещает заметку в архив или возвращает из него.
    pub fn set_note_archived(&self, id: &str, archived: bool) -> Result<Note> {
        self.update_note(
            id,
            NotePatch {
                archived: Some(archived),
                ..Default::default()
            },
        )
    }

    /// Перемещает заметку в папку (`None` — в корень).
    pub fn move_note(&self, id: &str, folder_id: Option<&str>) -> Result<Note> {
        self.update_note(
            id,
            NotePatch {
                folder_id: Some(folder_id.map(str::to_owned)),
                ..Default::default()
            },
        )
    }

    /// Групповое перемещение заметок; возвращает число перемещённых.
    pub fn move_notes(&self, ids: &[EntityId], folder_id: Option<&str>) -> Result<u32> {
        let mut moved = 0u32;
        for id in ids {
            self.move_note(id, folder_id)?;
            moved += 1;
        }
        Ok(moved)
    }

    /// Мягкое удаление: заметка попадает в корзину.
    pub fn delete_note(&self, id: &str) -> Result<()> {
        let now = Timestamp::now();
        self.transaction(|connection| {
            let key = note_key_by_uid(connection, id)?;
            let updated = connection.execute(
                "UPDATE notes SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
                params![key, now.millis()],
            )?;
            if updated == 0 {
                return Err(Error::not_found("note", id));
            }
            Ok(())
        })
    }

    /// Восстанавливает заметку из корзины.
    pub fn restore_note(&self, id: &str) -> Result<Note> {
        let now = Timestamp::now();
        self.transaction(|connection| {
            let key = note_key_by_uid(connection, id)?;
            connection.execute(
                "UPDATE notes SET deleted_at = NULL, updated_at = ?2 WHERE id = ?1",
                params![key, now.millis()],
            )?;
            Ok(())
        })?;
        self.get_note(id)
    }

    /// Безвозвратно удаляет заметку вместе с версиями и связями.
    pub fn purge_note(&self, id: &str) -> Result<()> {
        self.transaction(|connection| {
            let key = note_key_by_uid(connection, id)?;
            connection.execute("DELETE FROM notes WHERE id = ?1", params![key])?;
            Ok(())
        })
    }

    /// Очищает корзину; возвращает число удалённых заметок.
    pub fn empty_trash(&self) -> Result<u32> {
        self.transaction(|connection| {
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM notes WHERE deleted_at IS NOT NULL",
                [],
                |row| row.get(0),
            )?;
            connection.execute("DELETE FROM notes WHERE deleted_at IS NOT NULL", [])?;
            Ok(u32::try_from(count).unwrap_or(u32::MAX))
        })
    }

    /// Удаляет из корзины заметки старше `days` суток.
    pub fn purge_expired_trash(&self, days: u32) -> Result<u32> {
        let ids = self.trash_expired_ids(days)?;
        for id in &ids {
            self.purge_note(id)?;
        }
        Ok(ids.len() as u32)
    }

    /// Создаёт копию заметки в той же папке.
    pub fn duplicate_note(&self, id: &str) -> Result<Note> {
        let source = self.get_note(id)?;
        self.create_note(NoteDraft {
            title: Some(truncate_chars(
                &format!("{} (копия)", source.title),
                limits::MAX_TITLE_CHARS,
            )),
            body: Some(source.body.clone()),
            folder_id: source.folder_id.clone(),
            tags: Vec::new(),
            pinned: Some(false),
            color: source.color.clone(),
            encrypted: Some(false),
        })
    }

    /// Создаёт папку.
    pub fn create_folder(&self, draft: FolderDraft) -> Result<Folder> {
        draft.validate()?;
        let name = truncate_chars(&normalize_label(&draft.name), limits::MAX_FOLDER_NAME_CHARS);
        let now = Timestamp::now();

        self.transaction(|connection| {
            let parent_key = match &draft.parent_id {
                Some(id) => Some(
                    folder_key_by_uid(connection, id)?
                        .ok_or_else(|| Error::not_found("folder", id.clone()))?,
                ),
                None => None,
            };
            let uid = new_id();
            let sort_order: i64 = connection
                .query_row(
                    "SELECT COALESCE(MAX(sort_order), 0) + 1 FROM folders
                     WHERE COALESCE(parent_id, 0) = COALESCE(?1, 0)",
                    params![parent_key],
                    |row| row.get(0),
                )
                .unwrap_or(1);

            ensure_name_is_free(connection, &name, parent_key, None)?;
            connection.execute(
                "INSERT INTO folders (uid, name, parent_id, color, icon, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![uid, name, parent_key, draft.color, draft.icon, sort_order, now.millis()],
            )?;
            read_folder(connection, &uid)
        })
    }

    /// Обновляет папку, не допуская появления цикла в дереве.
    pub fn update_folder(&self, id: &str, patch: FolderPatch) -> Result<Folder> {
        patch.validate()?;
        let now = Timestamp::now();

        self.transaction(|connection| {
            let key =
                folder_key_by_uid(connection, id)?.ok_or_else(|| Error::not_found("folder", id))?;

            if let Some(Some(new_parent_uid)) = patch.parent() {
                let new_parent = folder_key_by_uid(connection, &new_parent_uid)?
                    .ok_or_else(|| Error::not_found("folder", new_parent_uid))?;
                if new_parent == key {
                    return Err(Error::invalid(
                        "папка не может быть своей собственной родительской",
                    ));
                }
                // Проверяем, является ли ПЕРЕМЕЩАЕМАЯ папка предком нового
                // родителя: обход идёт вверх от new_parent, поэтому аргументы
                // именно в таком порядке. Перепутанный порядок пропускал цикл
                // (поддерево всегда «ниже» перемещаемой папки).
                if is_ancestor(connection, key, new_parent)? {
                    return Err(Error::invalid(
                        "нельзя переместить папку внутрь её собственного поддерева",
                    ));
                }
                connection.execute(
                    "UPDATE folders SET parent_id = ?2, updated_at = ?3 WHERE id = ?1",
                    params![key, new_parent, now.millis()],
                )?;
            } else if let Some(None) = patch.parent() {
                connection.execute(
                    "UPDATE folders SET parent_id = NULL, updated_at = ?2 WHERE id = ?1",
                    params![key, now.millis()],
                )?;
            }

            let mut sets: Vec<String> = Vec::new();
            let mut arguments: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(key)];

            if let Some(name) = &patch.name {
                let normalized =
                    truncate_chars(&normalize_label(name), limits::MAX_FOLDER_NAME_CHARS);
                let parent_key: Option<i64> = connection.query_row(
                    "SELECT parent_id FROM folders WHERE id = ?1",
                    params![key],
                    |row| row.get(0),
                )?;
                ensure_name_is_free(connection, &normalized, parent_key, Some(key))?;
                arguments.push(Box::new(normalized));
                sets.push(format!("name = ?{}", arguments.len()));
            }
            if let Some(color) = &patch.color {
                arguments.push(Box::new(color.clone()));
                sets.push(format!("color = ?{}", arguments.len()));
            }
            if let Some(icon) = &patch.icon {
                arguments.push(Box::new(icon.clone()));
                sets.push(format!("icon = ?{}", arguments.len()));
            }
            if let Some(sort_order) = patch.sort_order {
                arguments.push(Box::new(sort_order));
                sets.push(format!("sort_order = ?{}", arguments.len()));
            }
            if !sets.is_empty() {
                arguments.push(Box::new(now.millis()));
                sets.push(format!("updated_at = ?{}", arguments.len()));
                let sql = format!("UPDATE folders SET {} WHERE id = ?1", sets.join(", "));
                let references: Vec<&dyn rusqlite::types::ToSql> = arguments
                    .iter()
                    .map(|value| value.as_ref() as &dyn rusqlite::types::ToSql)
                    .collect();
                connection.execute(&sql, rusqlite::params_from_iter(references))?;
            }
            read_folder(connection, id)
        })
    }

    /// Перемещает папку (`None` — поднять в корень).
    pub fn move_folder(&self, id: &str, parent_id: Option<&str>) -> Result<Folder> {
        self.update_folder(
            id,
            FolderPatch {
                parent_id: Some(parent_id.map(str::to_owned)),
                ..Default::default()
            },
        )
    }

    /// Удаляет папку.
    ///
    /// Заметки не удаляются вместе с папкой: они поднимаются в корень, а
    /// вложенные папки — к родителю удалённой. Пользователь не ожидает потери
    /// данных от действия «удалить папку».
    pub fn delete_folder(&self, id: &str) -> Result<DeleteFolderOutcome> {
        self.transaction(|connection| {
            let key =
                folder_key_by_uid(connection, id)?.ok_or_else(|| Error::not_found("folder", id))?;
            let parent_key: Option<i64> = connection.query_row(
                "SELECT parent_id FROM folders WHERE id = ?1",
                params![key],
                |row| row.get(0),
            )?;

            let moved_notes = connection.execute(
                "UPDATE notes SET folder_id = NULL WHERE folder_id = ?1",
                params![key],
            )? as u32;
            let moved_folders = connection.execute(
                "UPDATE folders SET parent_id = ?2 WHERE parent_id = ?1",
                params![key, parent_key],
            )? as u32;
            connection.execute("DELETE FROM folders WHERE id = ?1", params![key])?;

            Ok(DeleteFolderOutcome {
                moved_notes,
                moved_folders,
            })
        })
    }

    /// Меняет порядок папок внутри одного родителя.
    ///
    /// Принимает идентификаторы в новом порядке и проставляет `sort_order` по
    /// индексу — drag-and-drop на фронтенде сводится к одному вызову.
    pub fn reorder_folders(&self, ordered_ids: &[EntityId]) -> Result<()> {
        let now = Timestamp::now();
        self.transaction(|connection| {
            for (index, id) in ordered_ids.iter().enumerate() {
                let key = folder_key_by_uid(connection, id)?
                    .ok_or_else(|| Error::not_found("folder", id.clone()))?;
                connection.execute(
                    "UPDATE folders SET sort_order = ?2, updated_at = ?3 WHERE id = ?1",
                    params![key, index as i64, now.millis()],
                )?;
            }
            Ok(())
        })
    }

    /// Создаёт тег.
    pub fn create_tag(&self, draft: TagDraft) -> Result<()> {
        draft.validate()?;
        let name = truncate_chars(&normalize_tag_name(&draft.name), limits::MAX_TAG_NAME_CHARS);
        let lower = name.to_lowercase();
        let uid = new_id();
        let now = Timestamp::now();
        let pooled = self.connection()?;
        let inserted = pooled.get().execute(
            "INSERT OR IGNORE INTO tags (uid, name, name_lower, color, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![uid, name, lower, draft.color, now.millis()],
        )?;
        if inserted == 0 {
            return Err(Error::Constraint(format!("тег «{name}» уже существует")));
        }
        Ok(())
    }

    /// Переименовывает тег; при совпадении имени с существующим — объединяет их.
    pub fn rename_tag(&self, id: &str, new_name: &str) -> Result<()> {
        let name = truncate_chars(&normalize_tag_name(new_name), limits::MAX_TAG_NAME_CHARS);
        if name.is_empty() {
            return Err(Error::invalid_field(
                "название тега не может быть пустым",
                "name",
            ));
        }
        let lower = name.to_lowercase();

        self.transaction(|connection| {
            let key = tag_key_by_uid(connection, id)?.ok_or_else(|| Error::not_found("tag", id))?;
            let existing: Option<i64> = connection
                .query_row(
                    "SELECT id FROM tags WHERE name_lower = ?1 AND id <> ?2",
                    params![lower, key],
                    |row| row.get(0),
                )
                .optional()?;

            match existing {
                // Слияние с существующим тегом: сначала переносим связи
                // (пропуская уже существующие пары), затем удаляем старый тег.
                // Менять имя до переноса нельзя — UNIQUE(name_lower) не даст.
                Some(target) => {
                    connection.execute(
                        "INSERT OR IGNORE INTO note_tags (note_id, tag_id)
                         SELECT note_id, ?2 FROM note_tags WHERE tag_id = ?1",
                        params![key, target],
                    )?;
                    connection.execute("DELETE FROM note_tags WHERE tag_id = ?1", params![key])?;
                    connection.execute("DELETE FROM tags WHERE id = ?1", params![key])?;
                }
                None => {
                    connection.execute(
                        "UPDATE tags SET name = ?2, name_lower = ?3 WHERE id = ?1",
                        params![key, name, lower],
                    )?;
                }
            }
            Ok(())
        })
    }

    /// Меняет цвет тега.
    pub fn set_tag_color(&self, id: &str, color: Option<&str>) -> Result<()> {
        if let Some(value) = color {
            if !limits::is_allowed_color(value) {
                return Err(Error::invalid_field("недопустимый цвет тега", "color"));
            }
        }
        let pooled = self.connection()?;
        let key = tag_key_by_uid(pooled.get(), id)?.ok_or_else(|| Error::not_found("tag", id))?;
        pooled.get().execute(
            "UPDATE tags SET color = ?2 WHERE id = ?1",
            params![key, color],
        )?;
        Ok(())
    }

    /// Удаляет тег; возвращает число заметок, потерявших метку.
    pub fn delete_tag(&self, id: &str) -> Result<u32> {
        self.transaction(|connection| {
            let key = tag_key_by_uid(connection, id)?.ok_or_else(|| Error::not_found("tag", id))?;
            let detached =
                connection.execute("DELETE FROM note_tags WHERE tag_id = ?1", params![key])? as u32;
            connection.execute("DELETE FROM tags WHERE id = ?1", params![key])?;
            Ok(detached)
        })
    }

    /// Привязывает заметку к тегу, создавая тег при необходимости.
    pub fn add_tag_to_note(&self, note_id: &str, tag_name: &str) -> Result<()> {
        self.transaction(|connection| {
            let note_key = note_key_by_uid(connection, note_id)?;
            let tag_ids = resolve_tags(connection, &[tag_name.to_owned()])?;
            link_tags(connection, note_key, &tag_ids)
        })
    }

    /// Отвязывает тег от заметки.
    pub fn remove_tag_from_note(&self, note_id: &str, tag_id: &str) -> Result<()> {
        self.transaction(|connection| {
            let note_key = note_key_by_uid(connection, note_id)?;
            let tag_key = tag_key_by_uid(connection, tag_id)?
                .ok_or_else(|| Error::not_found("tag", tag_id))?;
            connection.execute(
                "DELETE FROM note_tags WHERE note_id = ?1 AND tag_id = ?2",
                params![note_key, tag_key],
            )?;
            connection.execute(
                "UPDATE notes SET updated_at = ?2 WHERE id = ?1",
                params![note_key, Timestamp::now().millis()],
            )?;
            Ok(())
        })
    }

    // -----------------------------------------------------------------------
    // Сейф
    // -----------------------------------------------------------------------

    /// Инициализирует сейф: выводит ключ из пароля и сохраняет верификатор.
    ///
    /// После инициализации сейф считается разблокированным до явной блокировки
    /// или закрытия приложения — пароль вводился только что.
    pub fn vault_initialize(&self, password: &str) -> Result<()> {
        crypto::validate_password(password)?;
        let (record, key) = crypto::initialize(password)?;
        let now = Timestamp::now();
        self.transaction(|connection| {
            connection.execute(
                "INSERT INTO vault (id, kdf_params, verifier, created_at, updated_at)
                 VALUES (1, ?1, ?2, ?3, ?3)
                 ON CONFLICT(id) DO UPDATE SET kdf_params = ?1, verifier = ?2, updated_at = ?3",
                params![record.params_json()?, record.verifier, now.millis()],
            )?;
            Ok(())
        })?;
        self.vault.replace(record.verifier.clone(), key);
        Ok(())
    }

    /// Проверяет пароль, кэширует ключ сессии и возвращает его.
    pub fn vault_unlock(&self, password: &str) -> Result<Vec<u8>> {
        let pooled = self.connection()?;
        let record = read_vault_record(pooled.get())?.ok_or_else(|| {
            Error::vault(
                "сейф не инициализирован",
                crate::error::VaultErrorCode::NotInitialized,
            )
        })?;
        let key = crypto::unlock(&record, password)?;
        self.vault.replace(record.verifier.clone(), key.clone());
        Ok(key)
    }

    /// Ключ сессии сейфа для внешнего потребителя (синхронизация E2EE).
    ///
    /// Возвращает `None`, если сейф заблокирован: вызывающий код обязан решить,
    /// что делать — спросить пароль или отказаться от шифрованной выгрузки.
    /// Молча выгрузить открытый текст вместо шифрованного нельзя.
    pub fn vault_session_key(&self, verifier: &str) -> Option<Vec<u8>> {
        self.vault.key_for(verifier)
    }

    /// Ключ сессии для текущего сейфа (читает запись из базы).
    pub fn vault_current_key(&self) -> Result<Option<Vec<u8>>> {
        let pooled = self.connection()?;
        Ok(match read_vault_record(pooled.get())? {
            Some(record) => self.vault.key_for(&record.verifier),
            None => None,
        })
    }

    /// Заблокировать сейф: ключ сессии забывается.
    pub fn vault_lock(&self) {
        self.vault.clear();
    }

    /// Разблокирован ли сейф прямо сейчас.
    pub fn vault_is_unlocked(&self) -> Result<bool> {
        let pooled = self.connection()?;
        Ok(match read_vault_record(pooled.get())? {
            Some(record) => self.vault.key_for(&record.verifier).is_some(),
            None => false,
        })
    }

    /// Запись сейфа (параметры KDF и верификатор), если он инициализирован.
    pub fn vault_record(&self) -> Result<Option<VaultRecord>> {
        let pooled = self.connection()?;
        read_vault_record(pooled.get())
    }

    /// Разблокирован ли сейф и инициализирован ли он — одним запросом.
    pub fn vault_status(&self) -> Result<VaultStatus> {
        let pooled = self.connection()?;
        let record = read_vault_record(pooled.get())?;
        Ok(match &record {
            Some(record) => VaultStatus {
                initialized: true,
                unlocked: self.vault.key_for(&record.verifier).is_some(),
            },
            None => VaultStatus {
                initialized: false,
                unlocked: false,
            },
        })
    }

    /// Смена пароля сейфа: перешифровывает все зашифрованные заметки.
    pub fn vault_change_password(&self, current: &str, next: &str) -> Result<u32> {
        crypto::validate_password(next)?;
        // Запись сейфа читается внутри транзакции: отдельное обращение к пулу
        // означало бы два одновременно заимствованных соединения.
        let (new_record, new_key) = crypto::initialize(next)?;
        let now = Timestamp::now();

        let reencrypted = self.transaction(|connection| {
            let record = require_vault_record(connection)?;
            let old_key = crypto::unlock(&record, current)?;
            connection.execute(
                "UPDATE vault SET kdf_params = ?1, verifier = ?2, updated_at = ?3 WHERE id = 1",
                params![new_record.params_json()?, new_record.verifier, now.millis()],
            )?;
            let mut statement =
                connection.prepare("SELECT id, body FROM notes WHERE encrypted = 1")?;
            let rows: Vec<(i64, String)> = statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<std::result::Result<_, _>>()?;
            drop(statement);

            let mut count = 0u32;
            for (key, stored) in rows {
                let plain = crypto::decrypt_container(&old_key, &stored)?;
                let container = crypto::encrypt_container(&new_key, plain.as_bytes())?;
                connection.execute(
                    "UPDATE notes SET body = ?2, updated_at = ?3 WHERE id = ?1",
                    params![key, container, now.millis()],
                )?;
                count += 1;
            }
            Ok(count)
        })?;

        self.vault.replace(new_record.verifier.clone(), new_key);
        Ok(reencrypted)
    }

    /// Отключает сейф, расшифровывая все заметки обратно.
    pub fn vault_disable(&self, password: &str) -> Result<u32> {
        let decrypted = self.transaction(|connection| {
            let record = require_vault_record(connection)?;
            let key = crypto::unlock(&record, password)?;
            let mut statement =
                connection.prepare("SELECT id, body FROM notes WHERE encrypted = 1")?;
            let rows: Vec<(i64, String)> = statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<std::result::Result<_, _>>()?;
            drop(statement);

            let mut count = 0u32;
            for (note_key, stored) in rows {
                let plain = crypto::decrypt_container(&key, &stored)?;
                connection.execute(
                    "UPDATE notes SET body = ?2, encrypted = 0, updated_at = ?3 WHERE id = ?1",
                    params![note_key, plain, Timestamp::now().millis()],
                )?;
                count += 1;
            }
            connection.execute("DELETE FROM vault WHERE id = 1", [])?;
            Ok(count)
        })?;
        self.vault.clear();
        Ok(decrypted)
    }

    /// Переносит заметку в сейф.
    ///
    /// Тело зашифрованной заметки исключается из полнотекстового индекса:
    /// искать по содержимому, которое пользователь спрятал под пароль, было бы
    /// нарушением самой идеи сейфа. Заголовок остаётся в индексе — он нужен,
    /// чтобы заметку можно было найти в списке.
    pub fn encrypt_note(&self, id: &str) -> Result<()> {
        let vault = Arc::clone(&self.vault);
        self.transaction(|connection| {
            let note_key = note_key_by_uid(connection, id)?;
            let record = read_vault_record(connection)?.ok_or_else(|| {
                Error::vault(
                    "сейф не инициализирован",
                    crate::error::VaultErrorCode::NotInitialized,
                )
            })?;
            let key = unlocked_key(&vault, &record)?;
            let (body, encrypted): (String, i64) = connection.query_row(
                "SELECT body, encrypted FROM notes WHERE id = ?1",
                params![note_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            if encrypted != 0 {
                return Ok(());
            }
            let container = crypto::encrypt_container(&key, body.as_bytes())?;
            // Триггер notes_au учитывает флаг encrypted и кладёт в индекс
            // пустое тело — содержимое сейфа не должно находиться поиском.
            connection.execute(
                "UPDATE notes SET body = ?2, encrypted = 1, updated_at = ?3 WHERE id = ?1",
                params![note_key, container, Timestamp::now().millis()],
            )?;
            Ok(())
        })
    }

    /// Выводит заметку из сейфа, возвращая её тело в поисковый индекс.
    pub fn decrypt_note(&self, id: &str) -> Result<()> {
        let vault = Arc::clone(&self.vault);
        self.transaction(|connection| {
            let note_key = note_key_by_uid(connection, id)?;
            let record = read_vault_record(connection)?.ok_or_else(|| {
                Error::vault(
                    "сейф не инициализирован",
                    crate::error::VaultErrorCode::NotInitialized,
                )
            })?;
            let key = unlocked_key(&vault, &record)?;
            let (body, encrypted): (String, i64) = connection.query_row(
                "SELECT body, encrypted FROM notes WHERE id = ?1",
                params![note_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            if encrypted == 0 {
                return Ok(());
            }
            let plain = crypto::decrypt_container(&key, &body)?;
            // Триггер вернёт в индекс текст без разметки.
            connection.execute(
                "UPDATE notes SET body = ?2, encrypted = 0, updated_at = ?3 WHERE id = ?1",
                params![note_key, plain, Timestamp::now().millis()],
            )?;
            Ok(())
        })
    }

    /// Читает тело заметки, при необходимости расшифровывая его.
    ///
    /// Единственная точка, где приложение видит открытый текст зашифрованной
    /// заметки: если сейф заблокирован, возвращается ошибка, а не пустая строка.
    pub fn read_note_body(&self, id: &str) -> Result<String> {
        let vault = Arc::clone(&self.vault);
        let pooled = self.connection()?;
        let connection = pooled.get();
        let note_key = note_key_by_uid(connection, id)?;
        let (body, encrypted): (String, i64) = connection.query_row(
            "SELECT body, encrypted FROM notes WHERE id = ?1",
            params![note_key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if encrypted == 0 {
            return Ok(body);
        }
        let record = read_vault_record(connection)?.ok_or_else(|| {
            Error::vault(
                "сейф не инициализирован",
                crate::error::VaultErrorCode::NotInitialized,
            )
        })?;
        crypto::decrypt_container(&unlocked_key(&vault, &record)?, &body)
    }

    /// Устанавливает запись сейфа и ключ сессии.
    ///
    /// Служебный метод: нужен тестам и сценариям восстановления, где ключ
    /// выводится не из введённого пароля, а из сохранённой резервной копии
    /// параметров. В интерфейсе приложения не используется — там только
    /// [`Store::vault_initialize`] и [`Store::vault_unlock`].
    pub fn vault_install_record_for_tests(&self, record: &VaultRecord, key: Vec<u8>) -> Result<()> {
        let now = Timestamp::now();
        self.transaction(|connection| {
            connection.execute(
                "INSERT INTO vault (id, kdf_params, verifier, created_at, updated_at)
                 VALUES (1, ?1, ?2, ?3, ?3)
                 ON CONFLICT(id) DO UPDATE SET kdf_params = ?1, verifier = ?2, updated_at = ?3",
                params![record.params_json()?, record.verifier, now.millis()],
            )?;
            Ok(())
        })?;
        self.vault.replace(record.verifier.clone(), key);
        Ok(())
    }

    /// Тело заметки так, как оно лежит на диске (для проверки отсутствия утечек).
    ///
    /// Служебный метод: приложению он не нужен, зато тесты могут убедиться, что
    /// зашифрованная заметка не хранит открытый текст.
    pub fn stored_body_for_tests(&self, id: &str) -> Result<String> {
        let pooled = self.connection()?;
        let key = note_key_by_uid(pooled.get(), id)?;
        pooled
            .get()
            .query_row(
                "SELECT body FROM notes WHERE id = ?1",
                params![key],
                |row| row.get(0),
            )
            .map_err(Error::from)
    }
}

/// Результат удаления папки.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteFolderOutcome {
    /// Сколько заметок поднято в корень.
    pub moved_notes: u32,
    /// Сколько вложенных папок поднято к родителю.
    pub moved_folders: u32,
}

// ---------------------------------------------------------------------------
// Вспомогательные функции (работают внутри транзакции)
// ---------------------------------------------------------------------------

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

fn tag_key_by_uid(connection: &Connection, uid: &str) -> Result<Option<i64>> {
    connection
        .query_row("SELECT id FROM tags WHERE uid = ?1", params![uid], |row| {
            row.get(0)
        })
        .optional()
        .map_err(Error::from)
}

fn existing_folder_key(connection: &Connection, note_key: i64) -> Option<i64> {
    connection
        .query_row(
            "SELECT folder_id FROM notes WHERE id = ?1",
            params![note_key],
            |row| row.get::<_, Option<i64>>(0),
        )
        .ok()
        .flatten()
}

fn read_note(connection: &Connection, uid: &str) -> Result<Note> {
    let sql = format!(
        "SELECT {NOTE_COLUMNS} FROM notes n LEFT JOIN folders f ON f.id = n.folder_id WHERE n.uid = ?1"
    );
    connection
        .query_row(&sql, params![uid], crate::db::read::map_note)
        .optional()?
        .ok_or_else(|| Error::not_found("note", uid))
}

fn read_folder(connection: &Connection, uid: &str) -> Result<Folder> {
    connection
        .query_row(
            "SELECT f.uid, f.name, p.uid, f.color, f.icon, f.sort_order, f.created_at, f.updated_at, \
                    (SELECT COUNT(*) FROM notes n WHERE n.folder_id = f.id AND n.deleted_at IS NULL AND n.archived = 0) \
             FROM folders f LEFT JOIN folders p ON p.id = f.parent_id WHERE f.uid = ?1",
            params![uid],
            |row| {
                Ok(Folder {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    parent_id: row.get(2)?,
                    color: row.get(3)?,
                    icon: row.get(4)?,
                    sort_order: row.get(5)?,
                    created_at: Timestamp::from_millis(row.get(6)?),
                    updated_at: Timestamp::from_millis(row.get(7)?),
                    note_count: row.get::<_, i64>(8)?.max(0) as u32,
                })
            },
        )
        .optional()?
        .ok_or_else(|| Error::not_found("folder", uid))
}

fn tag_names_of(connection: &Connection, note_key: i64) -> Result<Vec<String>> {
    let mut statement = connection.prepare(
        "SELECT t.name FROM note_tags nt JOIN tags t ON t.id = nt.tag_id \
         WHERE nt.note_id = ?1 ORDER BY lower(t.name)",
    )?;
    let rows = statement.query_map(params![note_key], |row| row.get::<_, String>(0))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::from)
}

/// Находит или создаёт теги по названиям; возвращает их внутренние ключи.
fn resolve_tags(connection: &Connection, names: &[String]) -> Result<Vec<i64>> {
    let mut keys: Vec<i64> = Vec::new();
    let now = Timestamp::now();

    for raw in names {
        let name = truncate_chars(&normalize_tag_name(raw), limits::MAX_TAG_NAME_CHARS);
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

        let key = match existing {
            Some(key) => key,
            None => {
                connection.execute(
                    "INSERT INTO tags (uid, name, name_lower, color, created_at) VALUES (?1, ?2, ?3, NULL, ?4)",
                    params![new_id(), name, lower, now.millis()],
                )?;
                connection.query_row("SELECT last_insert_rowid()", [], |row| row.get(0))?
            }
        };
        if !keys.contains(&key) {
            keys.push(key);
        }
        if keys.len() > limits::MAX_TAGS_PER_NOTE {
            return Err(Error::invalid_field(
                format!("не более {} тегов у заметки", limits::MAX_TAGS_PER_NOTE),
                "tags",
            ));
        }
    }
    Ok(keys)
}

fn link_tags(connection: &Connection, note_key: i64, tag_keys: &[i64]) -> Result<()> {
    for tag_key in tag_keys {
        connection.execute(
            "INSERT OR IGNORE INTO note_tags (note_id, tag_id) VALUES (?1, ?2)",
            params![note_key, tag_key],
        )?;
    }
    Ok(())
}

/// Проверяет, является ли `candidate` предком `target` (включая совпадение).
///
/// Обход идёт вверх от `target` по `parent_id`, поэтому для проверки «нельзя
/// переместить папку X внутрь её потомка Y» вызывать нужно как
/// `is_ancestor(conn, X, Y)`.
///
/// Ограничение глубины защищает от зацикливания на повреждённых данных: если в
/// базе уже есть цикл, обход останавливается вместо того, чтобы крутиться вечно.
fn is_ancestor(connection: &Connection, candidate: i64, target: i64) -> Result<bool> {
    let mut current = Some(target);
    let mut guard = 0usize;
    while let Some(key) = current {
        guard += 1;
        if guard > limits::MAX_FOLDER_DEPTH as usize * 4 {
            // Защита от повреждённых данных: дальше не идём.
            return Ok(false);
        }
        if key == candidate {
            return Ok(true);
        }
        current = connection
            .query_row(
                "SELECT parent_id FROM folders WHERE id = ?1",
                params![key],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten();
    }
    Ok(false)
}

/// Проверяет, что в этом родителе ещё нет папки с таким именем.
///
/// Сравнение без учёта регистра: «Работа» и «работа» в одной ветке — это одна
/// папка с точки зрения пользователя, и вторая запись только запутала бы поиск.
fn ensure_name_is_free(
    connection: &Connection,
    name: &str,
    parent_key: Option<i64>,
    self_key: Option<i64>,
) -> Result<()> {
    // Сравнение выполняется в Rust: `lower()` и `COLLATE NOCASE` в SQLite
    // работают только с ASCII, а имена папок у нас кириллические.
    let wanted = name.to_lowercase();
    let mut statement = match parent_key {
        Some(_) => connection.prepare("SELECT id, name FROM folders WHERE parent_id = ?1")?,
        None => connection.prepare("SELECT id, name FROM folders WHERE parent_id IS NULL")?,
    };
    let mapper = |row: &rusqlite::Row<'_>| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?));
    let siblings: Vec<(i64, String)> = match parent_key {
        Some(parent) => statement
            .query_map(params![parent], mapper)?
            .collect::<std::result::Result<_, _>>()?,
        None => statement
            .query_map([], mapper)?
            .collect::<std::result::Result<_, _>>()?,
    };

    let taken = siblings
        .iter()
        .any(|(key, existing)| Some(*key) != self_key && existing.to_lowercase() == wanted);
    if taken {
        return Err(Error::invalid_field(
            "папка с таким названием уже есть в этом месте",
            "name",
        ));
    }
    Ok(())
}

/// Определяет, должно ли тело заметки лежать в сейфе.
///
/// Зашифрованными бывают только заметки в хранилище с инициализированным
/// сейфом. Если сейфа нет, флаг принудительно сбрасывается — иначе заметка
/// осталась бы нечитаемой после восстановления из резервной копии.
fn vault_mode(record: &Option<VaultRecord>, requested: Option<bool>) -> Result<bool> {
    match (record.is_some(), requested.unwrap_or(false)) {
        (false, true) => Err(Error::vault(
            "сейф не инициализирован: задайте пароль в настройках, чтобы шифровать заметки",
            crate::error::VaultErrorCode::NotInitialized,
        )),
        (false, false) => Ok(false),
        (true, requested) => Ok(requested),
    }
}

/// Ключ разблокированного сейфа из кэша сессии.
fn unlocked_key(vault: &VaultSession, record: &VaultRecord) -> Result<Vec<u8>> {
    vault.key_for(&record.verifier).ok_or_else(|| {
        Error::vault(
            "сейф заблокирован — разблокируйте его, чтобы работать с зашифрованными заметками",
            crate::error::VaultErrorCode::BadPassword,
        )
    })
}

/// Состояние сейфа для интерфейса.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    /// Сейф создан.
    pub initialized: bool,
    /// Ключ сессии в памяти (заметки читаются).
    pub unlocked: bool,
}

/// Читает запись сейфа и требует её наличия.
fn require_vault_record(connection: &Connection) -> Result<VaultRecord> {
    read_vault_record(connection)?.ok_or_else(|| {
        Error::vault(
            "сейф не инициализирован",
            crate::error::VaultErrorCode::NotInitialized,
        )
    })
}

pub(crate) fn read_vault_record(connection: &Connection) -> Result<Option<VaultRecord>> {
    let row: Option<(String, String)> = connection
        .query_row(
            "SELECT kdf_params, verifier FROM vault WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match row {
        Some((params_json, verifier)) => Ok(Some(VaultRecord::from_parts(params_json, verifier)?)),
        None => Ok(None),
    }
}
