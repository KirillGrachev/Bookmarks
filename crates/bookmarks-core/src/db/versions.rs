//! Операции с историей версий на уровне хранилища.
//!
//! Сами снапшоты создаёт слой записи (`db::write`) перед каждым изменением
//! содержимого; здесь — то, что нужно интерфейсу: список, просмотр, откат и
//! очистка по политике из настроек.

use rusqlite::{params, OptionalExtension};

use crate::db::Store;
use crate::domain::note::{content_hash, count_words, Note, NotePatch};
use crate::domain::settings::VersionSettings;
use crate::error::{Error, Result};
use crate::time::Timestamp;
use crate::versions::{self, NoteVersion, VersionOrigin, VersionSummary};

impl Store {
    /// Список версий заметки (новые сверху), без тел.
    pub fn list_versions(&self, note_id: &str, limit: u32) -> Result<Vec<VersionSummary>> {
        let pooled = self.connection()?;
        let key = note_key(pooled.get(), note_id)?;
        versions::list_summaries(pooled.get(), key, limit)
    }

    /// Одна версия целиком.
    pub fn get_version(&self, version_id: i64) -> Result<NoteVersion> {
        let pooled = self.connection()?;
        versions::get(pooled.get(), version_id)?
            .ok_or_else(|| Error::not_found("version", version_id.to_string()))
    }

    /// Откат заметки к выбранной версии.
    ///
    /// Текущее состояние сохраняется как версия с признаком `restore`, поэтому
    /// откат обратим — случайное нажатие «восстановить» не теряет текст.
    pub fn restore_version(&self, version_id: i64) -> Result<Note> {
        let version = self.get_version(version_id)?;
        self.transaction(|connection| {
            let key = note_key(connection, &version.note_id)?;
            let (title, body): (String, String) = connection.query_row(
                "SELECT title, body FROM notes WHERE id = ?1",
                params![key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            versions::save_version(connection, key, &title, &body, VersionOrigin::Restore)?;
            Ok(())
        })?;

        self.update_note(
            &version.note_id,
            NotePatch {
                title: Some(version.title.clone()),
                body: Some(version.body.clone()),
                ..Default::default()
            },
        )
    }

    /// Удаляет одну версию.
    pub fn delete_version(&self, version_id: i64) -> Result<u32> {
        let pooled = self.connection()?;
        versions::delete(pooled.get(), version_id)
    }

    /// Чистит историю по политике настроек; возвращает число удалённых версий.
    pub fn prune_versions(&self, settings: &VersionSettings) -> Result<u32> {
        let pooled = self.connection()?;
        versions::prune(pooled.get(), settings)
    }

    /// Сохраняет версию явно (импорт, применение резервной копии).
    pub fn save_version(&self, note_id: &str, origin: VersionOrigin) -> Result<bool> {
        let note = self.get_note(note_id)?;
        let pooled = self.connection()?;
        let key = note_key(pooled.get(), note_id)?;
        versions::save_version(pooled.get(), key, &note.title, &note.body, origin)
    }

    /// Пересчитывает производные поля всех заметок (хеш и число слов).
    ///
    /// Нужно после восстановления из резервной копии чужого формата и после
    /// сбоя, который мог оставить несогласованные значения.
    pub fn recompute_derived_fields(&self) -> Result<u32> {
        let snapshot = self.sync_snapshot()?;
        let mut updated = 0u32;
        for note in snapshot {
            // Зашифрованные заметки не трогаем: их тело — контейнер, и хеш
            // от открытого текста был бы неверен.
            if note.deleted || note.encrypted {
                continue;
            }
            let hash = content_hash(&note.title, &note.body);
            let words = count_words(&note.body);
            let stored_words: i64 = {
                let pooled = self.connection()?;
                pooled.get().query_row(
                    "SELECT word_count FROM notes WHERE uid = ?1",
                    params![note.id],
                    |row| row.get(0),
                )?
            };
            if hash == note.content_hash && stored_words == i64::from(words) {
                continue;
            }
            let pooled = self.connection()?;
            pooled.get().execute(
                "UPDATE notes SET content_hash = ?2, word_count = ?3, updated_at = ?4 WHERE uid = ?1",
                params![note.id, hash, i64::from(words), Timestamp::now().millis()],
            )?;
            updated += 1;
        }
        Ok(updated)
    }
}

fn note_key(connection: &rusqlite::Connection, note_id: &str) -> Result<i64> {
    connection
        .query_row(
            "SELECT id FROM notes WHERE uid = ?1",
            params![note_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| Error::not_found("note", note_id))
}
