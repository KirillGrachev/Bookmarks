//! История версий заметки.
//!
//! Версия — снапшот заголовка и тела в момент изменения. Снимается автоматически
//! перед обновлением (см. `db::write::update_note`), при импорте и при
//! применении удалённых изменений синхронизации, а также явно перед откатом —
//! чтобы откат сам был откатываем.
//!
//! Хранилище чистится по политике из настроек: либо последние N версий на
//! заметку, либо всё не старше D дней. Без очистки база росла бы навсегда при
//! активном наборе текста.

use rusqlite::{params, Connection};

use crate::domain::note::content_hash;
use crate::domain::settings::VersionPolicy;
use crate::domain::VersionSettings;
use crate::error::Result;
use crate::ids::EntityId;
use crate::time::Timestamp;

/// Причина появления версии.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionOrigin {
    /// Обычная правка в редакторе.
    Edit,
    /// Импорт файла или восстановление из резервной копии.
    Import,
    /// Изменение пришло из удалённого хранилища.
    Sync,
    /// Версия создана перед откатом к предыдущей.
    Restore,
}

impl VersionOrigin {
    /// Строковое значение для базы.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Edit => "edit",
            Self::Import => "import",
            Self::Sync => "sync",
            Self::Restore => "restore",
        }
    }

    /// Разбор строкового значения; неизвестное считается правкой.
    pub fn parse(value: &str) -> Self {
        match value {
            "import" => Self::Import,
            "sync" => Self::Sync,
            "restore" => Self::Restore,
            _ => Self::Edit,
        }
    }
}

/// Версия заметки.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteVersion {
    /// Идентификатор версии.
    pub id: i64,
    /// Заметка.
    pub note_id: EntityId,
    /// Заголовок на момент снапшота.
    pub title: String,
    /// Тело на момент снапшота.
    pub body: String,
    /// Хеш содержимого — по нему отсекаются повторяющиеся снапшоты.
    pub content_hash: String,
    /// Число слов.
    pub word_count: u32,
    /// Момент создания версии.
    pub created_at: Timestamp,
    /// Причина появления.
    pub origin: VersionOrigin,
}

/// Краткое описание версии для списка (без тела).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSummary {
    /// Идентификатор версии.
    pub id: i64,
    /// Заголовок.
    pub title: String,
    /// Число слов.
    pub word_count: u32,
    /// Момент создания.
    pub created_at: Timestamp,
    /// Причина появления.
    pub origin: VersionOrigin,
    /// Хеш содержимого — для сравнения с текущим состоянием заметки.
    pub content_hash: String,
}

/// Сохраняет версию, если содержимое действительно изменилось.
///
/// Возвращает `true`, когда снапшот создан. Повторная запись того же текста
/// (автосейв сработал дважды подряд) версию не создаёт — иначе история
/// заполнилась бы одинаковыми строками за минуты.
pub fn save_version(
    connection: &Connection,
    note_key: i64,
    title: &str,
    body: &str,
    origin: VersionOrigin,
) -> Result<bool> {
    let hash = content_hash(title, body);
    let last: Option<String> = connection
        .query_row(
            "SELECT content_hash FROM note_versions WHERE note_id = ?1 ORDER BY created_at DESC, id DESC LIMIT 1",
            params![note_key],
            |row| row.get(0),
        )
        .unwrap_or(None);
    if last.as_deref() == Some(hash.as_str()) {
        return Ok(false);
    }

    connection.execute(
        "INSERT INTO note_versions (note_id, title, body, content_hash, word_count, created_at, origin)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            note_key,
            title,
            body,
            hash,
            i64::from(crate::domain::note::count_words(body)),
            Timestamp::now().millis(),
            origin.as_str(),
        ],
    )?;
    Ok(true)
}

/// Список версий заметки (новые сверху), без тел.
pub fn list_summaries(
    connection: &Connection,
    note_key: i64,
    limit: u32,
) -> Result<Vec<VersionSummary>> {
    let mut statement = connection.prepare(
        "SELECT v.id, v.title, v.word_count, v.created_at, v.origin, v.content_hash
         FROM note_versions v WHERE v.note_id = ?1 ORDER BY v.created_at DESC, v.id DESC LIMIT ?2",
    )?;
    let rows = statement.query_map(params![note_key, limit], |row| {
        let origin: String = row.get(4)?;
        Ok(VersionSummary {
            id: row.get(0)?,
            title: row.get(1)?,
            word_count: row.get::<_, i64>(2)?.max(0) as u32,
            created_at: Timestamp::from_millis(row.get(3)?),
            origin: VersionOrigin::parse(&origin),
            content_hash: row.get(5)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Одна версия целиком.
pub fn get(connection: &Connection, version_id: i64) -> Result<Option<NoteVersion>> {
    let mut statement = connection.prepare(
        "SELECT v.id, n.uid, v.title, v.body, v.content_hash, v.word_count, v.created_at, v.origin
         FROM note_versions v JOIN notes n ON n.id = v.note_id WHERE v.id = ?1",
    )?;
    let mut rows = statement.query_map(params![version_id], |row| {
        let origin: String = row.get(7)?;
        Ok(NoteVersion {
            id: row.get(0)?,
            note_id: row.get(1)?,
            title: row.get(2)?,
            body: row.get(3)?,
            content_hash: row.get(4)?,
            word_count: row.get::<_, i64>(5)?.max(0) as u32,
            created_at: Timestamp::from_millis(row.get(6)?),
            origin: VersionOrigin::parse(&origin),
        })
    })?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// Число версий заметки.
pub fn count_for_note(connection: &Connection, note_key: i64) -> Result<i64> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM note_versions WHERE note_id = ?1",
            params![note_key],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

/// Удаляет конкретную версию.
pub fn delete(connection: &Connection, version_id: i64) -> Result<u32> {
    let removed = connection.execute(
        "DELETE FROM note_versions WHERE id = ?1",
        params![version_id],
    )?;
    Ok(removed as u32)
}

/// Чистит историю по политике из настроек; возвращает число удалённых версий.
///
/// Выполняется одним SQL-запросом на заметку, а не циклом по версиям: на
/// библиотеке в десятки тысяч заметок разница — секунды против минут.
pub fn prune(connection: &Connection, settings: &VersionSettings) -> Result<u32> {
    let removed = match settings.policy {
        VersionPolicy::Off => connection.execute("DELETE FROM note_versions", [])?,
        VersionPolicy::KeepRecent => {
            let keep = i64::from(settings.keep_count.max(1));
            connection.execute(
                "DELETE FROM note_versions WHERE id NOT IN (
                    SELECT id FROM note_versions v
                    WHERE v.note_id = note_versions.note_id
                    ORDER BY v.created_at DESC, v.id DESC LIMIT ?1
                )",
                params![keep],
            )?
        }
        VersionPolicy::KeepDays => {
            let days = i64::from(settings.keep_days.max(1));
            let cutoff = Timestamp::now()
                .checked_add_days(-days)
                .map(|value| value.millis())
                .unwrap_or(0);
            connection.execute(
                "DELETE FROM note_versions WHERE created_at < ?1",
                params![cutoff],
            )?
        }
    };
    Ok(removed as u32)
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::*;
    use crate::db::schema::migrate;

    fn setup() -> (Connection, i64) {
        let mut connection = crate::db::open_test_connection().expect("db");
        migrate(&mut connection).expect("migrate");
        connection
            .execute(
                "INSERT INTO notes (uid, title, body, created_at, updated_at, content_hash)
                 VALUES ('note-1', 'Первая', 'текст', 1700000000000, 1700000000000, 'h1')",
                [],
            )
            .expect("seed note");
        let key: i64 = connection
            .query_row("SELECT id FROM notes WHERE uid = 'note-1'", [], |row| {
                row.get(0)
            })
            .expect("key");
        (connection, key)
    }

    #[test]
    fn saves_and_skips_duplicates() {
        let (connection, key) = setup();
        assert!(
            save_version(&connection, key, "Первая", "текст", VersionOrigin::Edit).expect("save")
        );
        assert!(
            !save_version(&connection, key, "Первая", "текст", VersionOrigin::Edit).expect("save")
        );
        assert!(
            save_version(&connection, key, "Первая", "текст 2", VersionOrigin::Edit).expect("save")
        );
        assert_eq!(count_for_note(&connection, key).expect("count"), 2);
    }

    #[test]
    fn lists_newest_first_with_metadata() {
        let (connection, key) = setup();
        save_version(&connection, key, "A", "первый", VersionOrigin::Edit).expect("save");
        save_version(&connection, key, "A", "второй", VersionOrigin::Import).expect("save");
        let summaries = list_summaries(&connection, key, 10).expect("list");
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].origin, VersionOrigin::Import);
        assert!(summaries[0].created_at >= summaries[1].created_at);
    }

    #[test]
    fn restores_full_version_body() {
        let (connection, key) = setup();
        save_version(
            &connection,
            key,
            "Заголовок",
            "полное тело",
            VersionOrigin::Edit,
        )
        .expect("save");
        let summary = list_summaries(&connection, key, 1).expect("list").remove(0);
        let version = get(&connection, summary.id).expect("get").expect("exists");
        assert_eq!(version.body, "полное тело");
        assert_eq!(version.note_id, "note-1");
        assert_eq!(version.word_count, 2);
        assert!(get(&connection, summary.id + 999).expect("get").is_none());
    }

    #[test]
    fn prune_keeps_recent_n() {
        let (connection, key) = setup();
        for index in 0..5 {
            // Разные тела и разные метки времени, чтобы порядок был однозначным.
            connection
                .execute(
                    "INSERT INTO note_versions (note_id, title, body, content_hash, word_count, created_at, origin)
                     VALUES (?1, 'A', ?2, ?3, 1, ?4, 'edit')",
                    params![key, format!("тело {index}"), format!("h{index}"), 1_700_000_000_000_i64 + index * 1000],
                )
                .expect("insert");
        }
        let settings = VersionSettings {
            policy: VersionPolicy::KeepRecent,
            keep_count: 2,
            keep_days: 90,
        };
        assert_eq!(prune(&connection, &settings).expect("prune"), 3);
        assert_eq!(count_for_note(&connection, key).expect("count"), 2);
    }

    #[test]
    fn prune_by_age_and_off() {
        let (connection, key) = setup();
        connection
            .execute(
                "INSERT INTO note_versions (note_id, title, body, content_hash, word_count, created_at, origin)
                 VALUES (?1, 'A', 'старая', 'h-old', 1, 1000, 'edit')",
                params![key],
            )
            .expect("old version");
        save_version(&connection, key, "A", "свежая", VersionOrigin::Edit).expect("save");

        let by_age = VersionSettings {
            policy: VersionPolicy::KeepDays,
            keep_count: 50,
            keep_days: 30,
        };
        assert_eq!(prune(&connection, &by_age).expect("prune"), 1);
        assert_eq!(count_for_note(&connection, key).expect("count"), 1);

        let off = VersionSettings {
            policy: VersionPolicy::Off,
            keep_count: 50,
            keep_days: 30,
        };
        assert_eq!(prune(&connection, &off).expect("prune"), 1);
        assert_eq!(count_for_note(&connection, key).expect("count"), 0);
    }

    #[test]
    fn deletes_single_version() {
        let (connection, key) = setup();
        save_version(&connection, key, "A", "тело", VersionOrigin::Edit).expect("save");
        let id = list_summaries(&connection, key, 1).expect("list")[0].id;
        assert_eq!(delete(&connection, id).expect("delete"), 1);
        assert_eq!(delete(&connection, id).expect("delete"), 0);
    }

    #[test]
    fn versions_disappear_with_note() {
        let (connection, key) = setup();
        save_version(&connection, key, "A", "тело", VersionOrigin::Edit).expect("save");
        connection
            .execute("DELETE FROM notes WHERE id = ?1", params![key])
            .expect("delete note");
        let remaining: i64 = connection
            .query_row("SELECT COUNT(*) FROM note_versions", [], |row| row.get(0))
            .expect("count");
        assert_eq!(remaining, 0, "cascade must remove versions");
    }

    #[test]
    fn origin_roundtrips() {
        for origin in [
            VersionOrigin::Edit,
            VersionOrigin::Import,
            VersionOrigin::Sync,
            VersionOrigin::Restore,
        ] {
            assert_eq!(VersionOrigin::parse(origin.as_str()), origin);
        }
        assert_eq!(
            VersionOrigin::parse("что-то-неизвестное"),
            VersionOrigin::Edit
        );
    }
}
