//! Хранилище: SQLite + FTS5, миграции схемы, чтение и запись.
//!
//! Точка входа — [`Store`]: он владеет набором соединений и всеми операциями.
//! Модуль разбит по ответственности:
//! - [`schema`] — миграции (версионируемые, данные не пересоздаются);
//! - [`read`] — выборки и фильтры;
//! - [`write`] — создание, обновление, удаление;
//! - [`row`] — маппинг строк SQL в доменные модели.
//!
//! Все операции синхронные: обёртка Tauri вызывает их из пула блокирующих
//! задач, поэтому событийный цикл окна никогда не ждёт мьютекса базы
//! (тот же подход, что устранял «залипание» интерфейса при фоновой
//! синхронизации справочника).

pub mod read;
pub mod schema;
pub mod sql_functions;
pub mod sync;
pub mod versions;
pub mod write;

pub use read::{
    DuplicateGroup, FolderNode, LibraryStats, NoteFilter, NotePage, NoteQuery, Scope, SearchHit,
};
pub use sync::SyncNote;
pub use write::{DeleteFolderOutcome, VaultStatus};

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::crypto::VaultSession;
use crate::error::{Error, Result};

/// Соединение в памяти с зарегистрированными пользовательскими функциями.
///
/// Общий помощник для тестов: без него `CREATE TRIGGER` в миграции падает с
/// «no such function: mb_plain_text».
pub fn open_test_connection() -> Result<Connection> {
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(PRAGMAS)?;
    sql_functions::register(&connection)?;
    Ok(connection)
}

/// Прагмы, применяемые к каждому соединению.
const PRAGMAS: &str = "\
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA temp_store = MEMORY;
PRAGMA cache_size = -16000;
";

/// Хранилище заметок.
///
/// Внутри — пул соединений за мьютексом: SQLite в режиме WAL допускает
/// параллельные чтения, а записи сериализуются на уровне базы. Пул нужен,
/// чтобы несколько IPC-команд (поиск в списке + автосейв открытой заметки)
/// не выстраивались в очередь за одним соединением.
#[derive(Clone)]
pub struct Store {
    inner: Arc<Mutex<Vec<Connection>>>,
    /// Источник данных — для сообщений об ошибках и экрана «О программе».
    location: Arc<str>,
    /// Имя общей in-memory базы (`None` для файлового хранилища).
    shared_memory_name: Option<Arc<str>>,
    /// Ключ разблокированного сейфа живёт только в памяти процесса и теряется
    /// при блокировке: в файл базы не попадает ничего, что помогло бы его
    /// восстановить.
    pub(crate) vault: Arc<VaultSession>,
}

impl Drop for Store {
    fn drop(&mut self) {
        // Клоны хранилища делят одно состояние, поэтому чистим только когда
        // держателей больше не осталось.
        if Arc::strong_count(&self.inner) > 1 {
            return;
        }
        if let Some(name) = &self.shared_memory_name {
            // Общая in-memory база живёт, пока открыто хотя бы одно соединение;
            // без явного удаления она осталась бы в памяти процесса навсегда.
            if let Ok(connection) =
                Connection::open(format!("file:{name}?mode=memory&cache=shared"))
            {
                let _ = connection.execute_batch("PRAGMA writable_schema = ON; DELETE FROM main;");
            }
        }
    }
}

impl std::fmt::Debug for Store {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Store")
            .field("location", &self.location)
            .field(
                "pool_size",
                &self.inner.lock().map(|pool| pool.len()).unwrap_or(0),
            )
            .field("vault_unlocked", &self.vault.is_unlocked())
            .finish()
    }
}

/// Заимствованное соединение из пула; при `Drop` возвращается обратно.
pub struct PooledConnection {
    connection: Option<Connection>,
    pool: Arc<Mutex<Vec<Connection>>>,
}

impl PooledConnection {
    /// Доступ к соединению.
    pub fn get(&self) -> &Connection {
        self.connection
            .as_ref()
            .expect("connection taken after release")
    }

    /// Доступ к соединению для операций, требующих `&mut Connection`
    /// (миграции, транзакции).
    pub fn get_mut(&mut self) -> &mut Connection {
        self.connection
            .as_mut()
            .expect("connection taken after release")
    }
}

impl std::ops::Deref for PooledConnection {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        self.get()
    }
}

impl Drop for PooledConnection {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            // Если пул повреждён (panic в другом потоке), соединение просто
            // закрывается — следующее обращение создаст новое.
            if let Ok(mut pool) = self.pool.lock() {
                pool.push(connection);
            }
        }
    }
}

impl Store {
    /// Открывает (или создаёт) хранилище в файле и применяет миграции.
    ///
    /// Родительские каталоги создаются при необходимости. Файл открывается
    /// в режиме чтения-записи с созданием; повреждённая база не молча
    /// перезаписывается, а возвращает ошибку — решение о восстановлении
    /// принимает пользователь на экране диагностики.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let location: Arc<str> = path.to_string_lossy().into_owned().into();
        let mut connection = Self::connect(path)?;
        schema::migrate(&mut connection)?;
        let store = Self {
            inner: Arc::new(Mutex::new(vec![connection])),
            location,
            shared_memory_name: None,
            vault: Arc::new(VaultSession::default()),
        };
        store.optimize()?;
        Ok(store)
    }

    /// Хранилище в памяти — для тестов и демо-режима.
    ///
    /// Используется общая кэшированная база с уникальным именем, а не
    /// `:memory:`: у такого соединения база принадлежит одному соединению, и
    /// второе соединение из пула увидело бы пустую схему. Имя уникально, чтобы
    /// параллельные тесты не делили данные между собой.
    pub fn open_in_memory() -> Result<Self> {
        let name: Arc<str> = format!("bookmarks_test_{}", crate::ids::new_id()).into();
        let mut connection = Self::connect(format!("file:{name}?mode=memory&cache=shared"))?;
        schema::migrate(&mut connection)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(vec![connection])),
            location: Arc::from(":memory:"),
            shared_memory_name: Some(name),
            vault: Arc::new(VaultSession::default()),
        })
    }

    /// Путь к файлу базы (или `:memory:`).
    pub fn location(&self) -> &str {
        &self.location
    }

    /// Берёт соединение из пула, при необходимости создавая новое.
    pub fn connection(&self) -> Result<PooledConnection> {
        let pooled = {
            let mut pool = self.inner.lock().map_err(|_| {
                Error::Constraint("пул соединений заблокирован паникующим потоком".into())
            })?;
            pool.pop()
        };
        let connection = match pooled {
            Some(connection) => connection,
            // Пул может опустеть, когда несколько команд выполняются
            // параллельно: создаём ещё одно соединение к тому же источнику.
            None => match &self.shared_memory_name {
                Some(name) => Self::connect(format!("file:{name}?mode=memory&cache=shared"))?,
                None => Self::connect(Path::new(self.location.as_ref()))?,
            },
        };
        Ok(PooledConnection {
            connection: Some(connection),
            pool: Arc::clone(&self.inner),
        })
    }

    /// Открывает соединение.
    ///
    /// Принимает и обычный путь, и URI вида `file:имя?mode=memory&cache=shared`
    /// (`String` реализует `AsRef<Path>`, а SQLite разбирает URI сам).
    fn connect(target: impl AsRef<Path>) -> Result<Connection> {
        let connection = Connection::open(target)?;
        connection.execute_batch(PRAGMAS)?;
        // Функции регистрируются до миграций: триггеры индекса их используют.
        sql_functions::register(&connection)?;
        Ok(connection)
    }

    /// Разовая оптимизация после открытия: ANALYZE для планировщика и
    /// усечение WAL, чтобы файл не рос бесконечно.
    fn optimize(&self) -> Result<()> {
        let connection = self.connection()?;
        connection.execute_batch("PRAGMA optimize; PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    /// Выполняет операцию в транзакции.
    ///
    /// Транзакция нужна везде, где меняется больше одной строки (заметка +
    /// её теги, версия, поисковый индекс): без неё краш посередине оставил
    /// бы базу в противоречивом состоянии.
    pub fn transaction<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T>,
    ) -> Result<T> {
        let mut pooled = self.connection()?;
        let connection = pooled.get_mut();
        connection.execute_batch("BEGIN IMMEDIATE")?;
        let result = operation(connection);
        match &result {
            Ok(_) => connection.execute_batch("COMMIT")?,
            Err(_) => {
                // Откатываем и игнорируем ошибку отката: пользователю важна
                // исходная причина сбоя.
                let _ = connection.execute_batch("ROLLBACK");
            }
        }
        result
    }

    /// Целостность хранилища: `PRAGMA integrity_check` + `quick_check` FTS.
    ///
    /// Возвращает список проблем; пустой вектор означает, что база здорова.
    pub fn verify_integrity(&self) -> Result<Vec<String>> {
        let connection = self.connection()?;
        let mut problems = Vec::new();
        let mut statement = connection.prepare("PRAGMA integrity_check")?;
        let rows: Vec<String> = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<_, _>>()?;
        if rows.first().map(String::as_str) != Some("ok") {
            problems.extend(rows);
        }
        // Индекс FTS может разойтись с таблицей после аварийного завершения;
        // `integrity-check` — штатная проверка FTS5. Она выполняется через
        // execute: команда не возвращает строк, и query_row счёл бы это ошибкой.
        if let Err(error) =
            connection.execute_batch("INSERT INTO notes_fts(notes_fts) VALUES('integrity-check')")
        {
            problems.push(format!("notes_fts: {error}"));
        }
        Ok(problems)
    }

    /// Пересобирает поисковый индекс из таблицы заметок.
    ///
    /// Используется после восстановления из резервной копии чужого формата и
    /// при диагностике «поиск ничего не находит».
    pub fn rebuild_search_index(&self) -> Result<u32> {
        self.transaction(|connection| {
            connection.execute_batch("INSERT INTO notes_fts(notes_fts) VALUES('rebuild');")?;
            let indexed: u32 = connection.query_row(
                "SELECT COUNT(*) FROM notes_fts WHERE body IS NOT NULL",
                [],
                |row| row.get(0),
            )?;
            Ok(indexed)
        })
    }

    /// Служебная статистика хранилища для экрана «О программе».
    pub fn storage_stats(&self) -> Result<StorageStats> {
        let connection = self.connection()?;
        let count = |sql: &str| -> Result<i64> {
            connection
                .query_row(sql, [], |row| row.get::<_, i64>(0))
                .map_err(Error::from)
        };
        let page_count: i64 = connection.query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let page_size: i64 = connection.query_row("PRAGMA page_size", [], |row| row.get(0))?;
        Ok(StorageStats {
            notes: count("SELECT COUNT(*) FROM notes WHERE deleted_at IS NULL")?,
            notes_in_trash: count("SELECT COUNT(*) FROM notes WHERE deleted_at IS NOT NULL")?,
            archived_notes: count(
                "SELECT COUNT(*) FROM notes WHERE archived = 1 AND deleted_at IS NULL",
            )?,
            encrypted_notes: count(
                "SELECT COUNT(*) FROM notes WHERE encrypted = 1 AND deleted_at IS NULL",
            )?,
            folders: count("SELECT COUNT(*) FROM folders")?,
            tags: count("SELECT COUNT(*) FROM tags")?,
            versions: count("SELECT COUNT(*) FROM note_versions")?,
            attachments: count("SELECT COUNT(*) FROM attachments")?,
            database_bytes: page_count.max(0) as u64 * page_size.max(0) as u64,
        })
    }
}

/// Статистика хранилища.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStats {
    /// Активных заметок.
    pub notes: i64,
    /// Заметок в корзине.
    pub notes_in_trash: i64,
    /// Заметок в архиве.
    pub archived_notes: i64,
    /// Зашифрованных заметок.
    pub encrypted_notes: i64,
    /// Папок.
    pub folders: i64,
    /// Тегов.
    pub tags: i64,
    /// Сохранённых версий.
    pub versions: i64,
    /// Вложений.
    pub attachments: i64,
    /// Размер файла базы, байт.
    pub database_bytes: u64,
}
