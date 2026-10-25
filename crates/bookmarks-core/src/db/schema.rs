//! Схема базы и её миграции.
//!
//! Заметки — данные пользователя, поэтому схема версионируется миграциями
//! (в отличие от пересоздаваемого зеркала каталога в справочнике). Каждая
//! миграция добавляется в конец списка и никогда не правится задним числом:
//! приложение, открывшее базу на версии N, обязано уметь довести её до
//! текущей.
//!
//! Полнотекстовый поиск — FTS5 с портер-стеммингом для латиницы и префиксными
//! индексами; для кириллицы FTS5 токенизирует по пробелам и пунктуации, чего
//! достаточно для поиска «по началу слова» (`закуп` находит «закупки»).
//! Индекс поддерживается триггерами, поэтому запись в `notes` автоматически
//! обновляет `notes_fts`.

use rusqlite::Connection;

use crate::error::{Error, Result};

/// Текущая версия схемы (число применённых миграций).
pub const SCHEMA_VERSION: usize = 1;

/// Миграция 1 — базовая схема.
const V1__BASE: &str = r#"
CREATE TABLE folders (
    id          INTEGER PRIMARY KEY,
    uid         TEXT    NOT NULL UNIQUE,
    name        TEXT    NOT NULL,
    parent_id   INTEGER REFERENCES folders(id) ON DELETE SET NULL,
    color       TEXT,
    icon        TEXT,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE INDEX idx_folders_parent ON folders(parent_id, sort_order);
-- Уникальность имени папки внутри родителя (без учёта регистра) проверяется
-- в коде (`db::write::ensure_name_is_free`). В SQL её выразить нельзя:
-- `lower()` и `COLLATE NOCASE` в SQLite работают только с ASCII, а имена
-- папок у нас кириллические.
CREATE INDEX idx_folders_name ON folders(parent_id, name);

CREATE TABLE tags (
    id          INTEGER PRIMARY KEY,
    uid         TEXT    NOT NULL UNIQUE,
    name        TEXT    NOT NULL,
    name_lower  TEXT    NOT NULL,
    color       TEXT,
    created_at  INTEGER NOT NULL
);

CREATE UNIQUE INDEX idx_tags_name_lower ON tags(name_lower);

CREATE TABLE notes (
    id            INTEGER PRIMARY KEY,
    uid           TEXT    NOT NULL UNIQUE,
    title         TEXT    NOT NULL DEFAULT '',
    body          TEXT    NOT NULL DEFAULT '',
    folder_id     INTEGER REFERENCES folders(id) ON DELETE SET NULL,
    pinned        INTEGER NOT NULL DEFAULT 0,
    archived      INTEGER NOT NULL DEFAULT 0,
    color         TEXT,
    encrypted     INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    deleted_at    INTEGER,
    content_hash  TEXT    NOT NULL,
    word_count    INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_notes_updated ON notes(deleted_at, archived, updated_at DESC);
CREATE INDEX idx_notes_created ON notes(deleted_at, archived, created_at DESC);
CREATE INDEX idx_notes_folder ON notes(deleted_at, folder_id, updated_at DESC);
CREATE INDEX idx_notes_pinned ON notes(deleted_at, pinned DESC, updated_at DESC);
CREATE INDEX idx_notes_deleted ON notes(deleted_at) WHERE deleted_at IS NOT NULL;

CREATE TABLE note_tags (
    note_id  INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    tag_id   INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (note_id, tag_id)
);

CREATE INDEX idx_note_tags_tag ON note_tags(tag_id);

CREATE VIRTUAL TABLE notes_fts USING fts5(
    title,
    body,
    content='notes',
    content_rowid='id',
    tokenize='porter unicode61 remove_diacritics 2'
);

-- Индекс поддерживается триггерами: достаточно писать в notes.
--
-- В индекс попадает ТЕКСТ БЕЗ РАЗМЕТКИ (mb_plain_text), а не исходный
-- Markdown: поиск по «**важное**» должен находить слово «важное», а не
-- звёздочки. Для зашифрованных заметок в индекс кладётся пустая строка —
-- содержимое сейфа не ищется, хотя заголовок остаётся видимым в списке.
CREATE TRIGGER notes_ai AFTER INSERT ON notes BEGIN
    INSERT INTO notes_fts(rowid, title, body)
    VALUES (new.id, new.title, CASE WHEN new.encrypted = 1 THEN '' ELSE mb_plain_text(new.body) END);
END;

CREATE TRIGGER notes_ad AFTER DELETE ON notes BEGIN
    INSERT INTO notes_fts(notes_fts, rowid, title, body)
    VALUES ('delete', old.id, old.title,
            CASE WHEN old.encrypted = 1 THEN '' ELSE mb_plain_text(old.body) END);
END;

CREATE TRIGGER notes_au AFTER UPDATE OF title, body, encrypted ON notes BEGIN
    INSERT INTO notes_fts(notes_fts, rowid, title, body)
    VALUES ('delete', old.id, old.title,
            CASE WHEN old.encrypted = 1 THEN '' ELSE mb_plain_text(old.body) END);
    INSERT INTO notes_fts(rowid, title, body)
    VALUES (new.id, new.title, CASE WHEN new.encrypted = 1 THEN '' ELSE mb_plain_text(new.body) END);
END;

CREATE TABLE note_versions (
    id          INTEGER PRIMARY KEY,
    note_id     INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    title       TEXT    NOT NULL,
    body        TEXT    NOT NULL,
    content_hash TEXT   NOT NULL,
    word_count  INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    origin      TEXT    NOT NULL DEFAULT 'edit'
);

CREATE INDEX idx_versions_note ON note_versions(note_id, created_at DESC);

CREATE TABLE attachments (
    id          INTEGER PRIMARY KEY,
    uid         TEXT    NOT NULL UNIQUE,
    note_id     INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    filename    TEXT    NOT NULL,
    mime        TEXT    NOT NULL DEFAULT 'application/octet-stream',
    size        INTEGER NOT NULL DEFAULT 0,
    sha256      TEXT    NOT NULL,
    stored_path TEXT    NOT NULL,
    created_at  INTEGER NOT NULL
);

CREATE INDEX idx_attachments_note ON attachments(note_id, created_at);

CREATE TABLE vault (
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    kdf_params   TEXT    NOT NULL,
    verifier     TEXT    NOT NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);
"#;

/// Список миграций по порядку применения.
///
/// Новая миграция добавляется в конец; править уже применённые нельзя — база
/// пользователя, открытая новой версией приложения, должна уметь дойти до
/// текущей схемы из любого предыдущего состояния.
pub const MIGRATIONS: [&str; 1] = [V1__BASE];

/// Применяет все недостающие миграции.
///
/// Собственный мигратор вместо внешней библиотеки: он занимает двадцать строк,
/// не тянет зависимостей и явно контролирует `user_version` — именно его
/// читает диагностика и резервное копирование.
pub fn migrate(connection: &mut Connection) -> Result<()> {
    let current = current_version(connection)?;
    if current > MIGRATIONS.len() as i64 {
        return Err(Error::Migration(format!(
            "база данных создана более новой версией приложения (схема {current}, поддерживается до {})",
            MIGRATIONS.len()
        )));
    }

    for (index, sql) in MIGRATIONS.iter().enumerate() {
        let version = (index + 1) as i64;
        if current >= version {
            continue;
        }
        // На время миграции внешние ключи отключаются: стандартная практика
        // SQLite, иначе промежуточное состояние таблицы не проходит проверку.
        connection.execute_batch("PRAGMA foreign_keys = OFF; BEGIN IMMEDIATE;")?;
        let applied = connection.execute_batch(sql);
        match applied {
            Ok(()) => {
                connection.pragma_update(None, "user_version", version)?;
                connection.execute_batch("COMMIT; PRAGMA foreign_keys = ON;")?;
            }
            Err(error) => {
                let _ = connection.execute_batch("ROLLBACK; PRAGMA foreign_keys = ON;");
                return Err(Error::Migration(format!(
                    "миграция {version} не применена: {error}"
                )));
            }
        }
    }
    Ok(())
}

/// Текущая версия схемы в открытой базе (для диагностики).
pub fn current_version(connection: &Connection) -> Result<i64> {
    connection
        .query_row(
            "SELECT COALESCE((SELECT * FROM pragma_user_version()), 0)",
            [],
            |row| row.get(0),
        )
        .map_err(Error::from)
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::*;

    fn open() -> Connection {
        crate::db::open_test_connection().expect("in-memory db")
    }

    #[test]
    fn applies_to_empty_database() {
        let mut connection = open();
        migrate(&mut connection).expect("migrates");
        assert_eq!(current_version(&connection).expect("version"), 1);
    }

    #[test]
    fn is_idempotent() {
        let mut connection = open();
        migrate(&mut connection).expect("first");
        migrate(&mut connection).expect("second");
        migrate(&mut connection).expect("third");
        assert_eq!(
            current_version(&connection).expect("version"),
            SCHEMA_VERSION as i64
        );
    }

    #[test]
    fn rejects_database_from_future_version() {
        let mut connection = open();
        migrate(&mut connection).expect("migrates");
        connection
            .pragma_update(None, "user_version", MIGRATIONS.len() as i64 + 3)
            .expect("bump");
        let error = migrate(&mut connection).expect_err("must fail");
        assert!(matches!(error, Error::Migration(_)));
    }

    #[test]
    fn creates_expected_objects() {
        let mut connection = open();
        migrate(&mut connection).expect("migrates");
        let mut statement = connection
            .prepare("SELECT name FROM sqlite_master WHERE type IN ('table','index','trigger') ORDER BY name")
            .expect("prepare");
        let names: Vec<String> = statement
            .query_map([], |row| row.get(0))
            .expect("query")
            .collect::<std::result::Result<_, _>>()
            .expect("rows");
        for expected in [
            "notes",
            "folders",
            "tags",
            "note_tags",
            "notes_fts",
            "note_versions",
            "attachments",
            "vault",
            "notes_ai",
            "notes_ad",
            "notes_au",
        ] {
            assert!(
                names.iter().any(|name| name == expected),
                "missing {expected} in {names:?}"
            );
        }
    }

    #[test]
    fn folder_name_index_exists() {
        let mut connection = open();
        migrate(&mut connection).expect("migrates");
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_folders_name'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 1);
    }

    #[test]
    fn orphaning_parent_moves_folder_to_root() {
        let mut connection = open();
        migrate(&mut connection).expect("migrates");
        let now = 1_700_000_000_000_i64;
        connection
            .execute_batch(
                "INSERT INTO folders (uid, name, parent_id, sort_order, created_at, updated_at)
                 VALUES ('root', 'Проекты', NULL, 0, 1700000000000, 1700000000000);
                 INSERT INTO folders (uid, name, parent_id, sort_order, created_at, updated_at)
                 VALUES ('child', 'Сайт', (SELECT id FROM folders WHERE uid = 'root'), 0, 1700000000000, 1700000000000);",
            )
            .expect("seed");
        let _ = now;
        connection
            .execute("DELETE FROM folders WHERE uid = 'root'", [])
            .expect("delete parent");
        let parent: Option<i64> = connection
            .query_row(
                "SELECT parent_id FROM folders WHERE uid = 'child'",
                [],
                |row| row.get(0),
            )
            .expect("child survived");
        assert!(
            parent.is_none(),
            "child must become a root folder, got {parent:?}"
        );
    }

    #[test]
    fn fts_index_follows_note_changes_via_triggers() {
        let mut connection = open();
        migrate(&mut connection).expect("migrates");
        let insert = |uid: &str, title: &str, body: &str| {
            connection
                .execute(
                    "INSERT INTO notes (uid, title, body, created_at, updated_at, content_hash)
                     VALUES (?1, ?2, ?3, 1700000000000, 1700000000000, 'h-' || ?1)",
                    rusqlite::params![uid, title, body],
                )
                .expect("insert note");
        };
        insert("n1", "Список покупок", "молоко хлеб яйца");
        assert_eq!(
            count_matches(&connection, "покуп*"),
            1,
            "insert must populate the index"
        );

        connection
            .execute("UPDATE notes SET body = 'только кофе' WHERE uid = 'n1'", [])
            .expect("update");
        assert_eq!(
            count_matches(&connection, "молоко"),
            0,
            "old text must leave the index"
        );
        assert_eq!(
            count_matches(&connection, "кофе"),
            1,
            "new text must be indexed"
        );

        connection
            .execute(
                "UPDATE notes SET title = 'Кофейный список' WHERE uid = 'n1'",
                [],
            )
            .expect("retitle");
        assert_eq!(count_matches(&connection, "кофе*"), 1);

        connection
            .execute("DELETE FROM notes WHERE uid = 'n1'", [])
            .expect("delete");
        assert_eq!(
            count_matches(&connection, "кофе*"),
            0,
            "delete must clean the index"
        );
        // Индекс остаётся согласованным после полного цикла.
        connection
            .execute(
                "INSERT INTO notes_fts(notes_fts) VALUES('integrity-check')",
                [],
            )
            .expect("fts integrity");
    }

    fn count_matches(connection: &Connection, query: &str) -> i64 {
        connection
            .query_row(
                "SELECT COUNT(*) FROM notes_fts WHERE notes_fts MATCH ?1",
                [query],
                |row| row.get(0),
            )
            .unwrap_or(0)
    }
}
