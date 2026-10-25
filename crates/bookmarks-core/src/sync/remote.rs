//! Транспорты синхронизации.
//!
//! Интерфейс намеренно узкий: список файлов, чтение, запись, удаление. Всё
//! остальное (манифесты, слияние, конфликты) живёт в `engine` и от транспорта
//! не зависит, поэтому одну и ту же логику синхронизации проверяют тесты на
//! хранилище в памяти, а в проде она работает поверх папки или WebDAV.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Ошибка транспорта синхронизации.
///
/// `Clone` нужен тестовому транспорту: имитация обрыва связи повторяет одну и
/// ту же ошибку для всех операций, пока её не снимут.
#[derive(Debug, Clone, thiserror::Error)]
pub enum RemoteError {
    /// Файла нет.
    #[error("remote file not found: {0}")]
    NotFound(String),
    /// Хранилище недоступно (нет сети, диск отключён).
    #[error("remote is unreachable: {0}")]
    Offline(String),
    /// Отказано в доступе (неверный логин/пароль, нет прав).
    #[error("remote access denied: {0}")]
    Auth(String),
    /// Недопустимый путь (выход за пределы каталога синхронизации).
    #[error("invalid remote path: {0}")]
    InvalidPath(String),
    /// Ошибка ввода-вывода (текст сообщения: `io::Error` не клонируется).
    #[error("remote io error: {0}")]
    Io(String),
}

impl From<std::io::Error> for RemoteError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// Результат операции транспорта.
pub type RemoteResult<T> = std::result::Result<T, RemoteError>;

/// Ошибка разбора адреса удалённого хранилища.
#[derive(Debug, thiserror::Error)]
#[error("invalid remote location: {0}")]
pub struct RemoteKindError(pub String);

/// Описание файла в удалённом хранилище.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFile {
    /// Относительный путь внутри хранилища (всегда через `/`).
    pub path: String,
    /// Размер, байт.
    pub size: u64,
    /// Метка изменения в миллисекундах UTC (0, если транспорт её не отдаёт).
    pub modified_at: i64,
}

/// Транспорт удалённого хранилища.
///
/// Реализации обязаны быть потокобезопасными: синхронизация выполняется в
/// фоновой задаче, параллельно с командами интерфейса.
pub trait Remote: Send + Sync {
    /// Человекочитаемое описание (путь или URL) для журнала и настроек.
    fn describe(&self) -> String;

    /// Список файлов в каталоге (не рекурсивно).
    fn list(&self, directory: &str) -> RemoteResult<Vec<RemoteFile>>;

    /// Читает файл целиком.
    fn read(&self, path: &str) -> RemoteResult<Vec<u8>>;

    /// Пишет файл целиком, создавая родительские каталоги.
    fn write(&self, path: &str, data: &[u8]) -> RemoteResult<()>;

    /// Удаляет файл; отсутствие файла не считается ошибкой.
    fn delete(&self, path: &str) -> RemoteResult<()>;

    /// Существует ли файл.
    fn exists(&self, path: &str) -> RemoteResult<bool> {
        Ok(self.read(path).is_ok())
    }
}

// ---------------------------------------------------------------------------
// Хранилище в памяти — для тестов и демо-режима
// ---------------------------------------------------------------------------

/// Хранилище в оперативной памяти.
///
/// Используется в тестах синхронизации (два «устройства» обмениваются данными
/// через общий экземпляр) и в браузерном демо-режиме, где файловой системы нет.
#[derive(Debug, Default, Clone)]
pub struct MemoryRemote {
    files: std::sync::Arc<std::sync::Mutex<BTreeMap<String, Vec<u8>>>>,
    /// Принудительный сбой следующих операций — для проверки поведения при обрыве связи.
    failure: std::sync::Arc<std::sync::Mutex<Option<RemoteError>>>,
}

impl MemoryRemote {
    /// Пустое хранилище.
    pub fn new() -> Self {
        Self::default()
    }

    /// Число файлов (для тестов).
    pub fn len(&self) -> usize {
        self.files.lock().map(|files| files.len()).unwrap_or(0)
    }

    /// Пусто ли хранилище.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Пути всех файлов (для тестов).
    pub fn paths(&self) -> Vec<String> {
        self.files
            .lock()
            .map(|files| files.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Включает имитацию сбоя: все операции начинают завершаться этой ошибкой.
    ///
    /// Сбой постоянный, пока не вызван `fail_with(None)` — так проверяется
    /// поведение приложения при обрыве связи на всех шагах синхронизации.
    pub fn fail_with(&self, error: Option<RemoteError>) {
        if let Ok(mut guard) = self.failure.lock() {
            *guard = error;
        }
    }

    fn take_failure(&self) -> Option<RemoteError> {
        self.failure.lock().ok().and_then(|guard| guard.clone())
    }

    /// Доступ к таблице файлов; отравленный мьютекс считаем недоступностью
    /// хранилища, а не паникой — синхронизация в фоне не должна ронять окно.
    fn files(&self) -> RemoteResult<std::sync::MutexGuard<'_, BTreeMap<String, Vec<u8>>>> {
        self.files
            .lock()
            .map_err(|_| RemoteError::Offline("хранилище заблокировано".into()))
    }
}

impl Remote for MemoryRemote {
    fn describe(&self) -> String {
        "memory".to_owned()
    }

    fn list(&self, directory: &str) -> RemoteResult<Vec<RemoteFile>> {
        if let Some(error) = self.take_failure() {
            return Err(error);
        }
        let prefix = normalize_directory(directory);
        let files = self.files()?;
        Ok(files
            .iter()
            .filter(|(path, _)| path.starts_with(&prefix))
            .map(|(path, data)| RemoteFile {
                path: path.clone(),
                size: data.len() as u64,
                modified_at: 0,
            })
            .collect())
    }

    fn read(&self, path: &str) -> RemoteResult<Vec<u8>> {
        if let Some(error) = self.take_failure() {
            return Err(error);
        }
        let key = normalize_path(path)?;
        let files = self.files()?;
        files.get(&key).cloned().ok_or(RemoteError::NotFound(key))
    }

    fn write(&self, path: &str, data: &[u8]) -> RemoteResult<()> {
        if let Some(error) = self.take_failure() {
            return Err(error);
        }
        let key = normalize_path(path)?;
        let mut files = self.files()?;
        files.insert(key, data.to_vec());
        Ok(())
    }

    fn delete(&self, path: &str) -> RemoteResult<()> {
        if let Some(error) = self.take_failure() {
            return Err(error);
        }
        let key = normalize_path(path)?;
        let mut files = self.files()?;
        files.remove(&key);
        Ok(())
    }

    fn exists(&self, path: &str) -> RemoteResult<bool> {
        let key = normalize_path(path)?;
        let files = self.files()?;
        Ok(files.contains_key(&key))
    }
}

// ---------------------------------------------------------------------------
// Локальная или сетевая папка
// ---------------------------------------------------------------------------

/// Хранилище в каталоге файловой системы.
///
/// Подходит и для внешнего диска, и для сетевой папки (SMB/WebDAV-монтирование):
/// приложение работает с обычными путями, а особенности транспорта берёт на
/// себя операционная система.
#[derive(Debug, Clone)]
pub struct DirectoryRemote {
    root: PathBuf,
}

impl DirectoryRemote {
    /// Создаёт транспорт с проверкой и нормализацией корневого каталога.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, RemoteKindError> {
        let root = root.as_ref();
        if root.as_os_str().is_empty() {
            return Err(RemoteKindError("не указан каталог синхронизации".into()));
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Корневой каталог.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Преобразует относительный путь в абсолютный, не выпуская за пределы корня.
    ///
    /// Защита обязательна: пути приходят из манифестов, которые могут быть
    /// созданы на другом устройстве (или подменены), поэтому `../../` и
    /// абсолютные пути отвергаются.
    fn resolve(&self, path: &str) -> RemoteResult<PathBuf> {
        let normalized = normalize_path(path)?;
        let mut resolved = self.root.clone();
        for component in Path::new(&normalized).components() {
            match component {
                Component::Normal(part) => resolved.push(part),
                Component::CurDir => {}
                _ => return Err(RemoteError::InvalidPath(normalized)),
            }
        }
        if !resolved.starts_with(&self.root) {
            return Err(RemoteError::InvalidPath(normalized));
        }
        Ok(resolved)
    }
}

impl Remote for DirectoryRemote {
    fn describe(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    fn list(&self, directory: &str) -> RemoteResult<Vec<RemoteFile>> {
        let target = self
            .resolve(directory)
            .unwrap_or_else(|_| self.root.clone());
        if !target.exists() {
            return Ok(Vec::new());
        }
        let mut result = Vec::new();
        for entry in std::fs::read_dir(&target)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if !file_type.is_file() {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(&self.root)
                .map_err(|_| RemoteError::InvalidPath(entry.path().to_string_lossy().into_owned()))?
                .to_string_lossy()
                .replace('\\', "/");
            let metadata = entry.metadata()?;
            let modified_at = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as i64)
                .unwrap_or(0);
            result.push(RemoteFile {
                path: relative,
                size: metadata.len(),
                modified_at,
            });
        }
        result.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(result)
    }

    fn read(&self, path: &str) -> RemoteResult<Vec<u8>> {
        let resolved = self.resolve(path)?;
        match std::fs::read(&resolved) {
            Ok(data) => Ok(data),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(RemoteError::NotFound(path.to_owned()))
            }
            Err(error) => Err(RemoteError::Io(error.to_string())),
        }
    }

    fn write(&self, path: &str, data: &[u8]) -> RemoteResult<()> {
        let resolved = self.resolve(path)?;
        if let Some(parent) = resolved.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Запись через временный файл и переименование: процесс, убитый
        // посередине, не оставит в хранилище половинку заметки.
        let temporary = resolved.with_extension("part");
        std::fs::write(&temporary, data)?;
        std::fs::rename(&temporary, &resolved)?;
        Ok(())
    }

    fn delete(&self, path: &str) -> RemoteResult<()> {
        let resolved = self.resolve(path)?;
        match std::fs::remove_file(&resolved) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn exists(&self, path: &str) -> RemoteResult<bool> {
        Ok(self.resolve(path)?.exists())
    }
}

/// Приводит путь к виду `a/b/c` и отвергает попытки выйти наружу.
fn normalize_path(path: &str) -> RemoteResult<String> {
    let cleaned: Vec<&str> = path
        .split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    if cleaned.contains(&"..") {
        return Err(RemoteError::InvalidPath(path.to_owned()));
    }
    if path.starts_with('/') || path.contains(":\\") {
        return Err(RemoteError::InvalidPath(path.to_owned()));
    }
    if cleaned.is_empty() {
        return Err(RemoteError::InvalidPath(path.to_owned()));
    }
    Ok(cleaned.join("/"))
}

/// Каталог с завершающим `/` для префиксного сравнения.
fn normalize_directory(directory: &str) -> String {
    let trimmed = directory.trim_matches(['/', '\\']);
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_remote_roundtrips_files() {
        let remote = MemoryRemote::new();
        assert!(remote.is_empty());
        remote.write("notes/a.md", b"hello").expect("write");
        assert_eq!(remote.read("notes/a.md").expect("read"), b"hello");
        assert!(remote.exists("notes/a.md").expect("exists"));
        assert_eq!(remote.list("notes").expect("list").len(), 1);
        assert_eq!(remote.list("other").expect("list").len(), 0);
        remote.delete("notes/a.md").expect("delete");
        assert!(!remote.exists("notes/a.md").expect("exists"));
        // Удаление отсутствующего файла — не ошибка.
        remote.delete("notes/a.md").expect("delete again");
    }

    #[test]
    fn memory_remote_simulates_persistent_outage() {
        let remote = MemoryRemote::new();
        remote.fail_with(Some(RemoteError::Offline("нет сети".into())));
        // Сбой держится на всех операциях — как настоящий обрыв связи.
        assert!(matches!(
            remote.write("a/b.md", b"x"),
            Err(RemoteError::Offline(_))
        ));
        assert!(matches!(
            remote.read("a/b.md"),
            Err(RemoteError::Offline(_))
        ));
        assert!(matches!(remote.list("a"), Err(RemoteError::Offline(_))));
        assert_eq!(remote.len(), 0);

        remote.fail_with(None);
        remote.write("a/b.md", b"x").expect("write");
        assert_eq!(remote.len(), 1);
    }

    #[test]
    fn rejects_escapes_from_root() {
        assert!(matches!(
            normalize_path("../etc/passwd"),
            Err(RemoteError::InvalidPath(_))
        ));
        assert!(matches!(
            normalize_path("/etc/passwd"),
            Err(RemoteError::InvalidPath(_))
        ));
        assert!(matches!(
            normalize_path("C:\\Windows\\system32"),
            Err(RemoteError::InvalidPath(_))
        ));
        assert_eq!(
            normalize_path("notes\\sub\\a.md").expect("windows separators"),
            "notes/sub/a.md"
        );
        assert!(matches!(
            normalize_path(""),
            Err(RemoteError::InvalidPath(_))
        ));
    }

    #[test]
    fn directory_remote_resolves_inside_root_only() {
        let temporary = tempfile::tempdir().expect("tempdir");
        let remote = DirectoryRemote::new(temporary.path()).expect("remote");
        remote.write("notes/sub/a.md", b"data").expect("write");
        assert_eq!(remote.read("notes/sub/a.md").expect("read"), b"data");
        assert!(temporary.path().join("notes/sub/a.md").exists());
        assert!(matches!(
            remote.read("../secret.md"),
            Err(RemoteError::InvalidPath(_))
        ));
        assert!(matches!(
            remote.write("../../evil.md", b"x"),
            Err(RemoteError::InvalidPath(_))
        ));
        let listed = remote.list("notes/sub").expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].path, "notes/sub/a.md");
        assert_eq!(listed[0].size, 4);
    }

    #[test]
    fn directory_remote_survives_missing_paths() {
        let temporary = tempfile::tempdir().expect("tempdir");
        let remote = DirectoryRemote::new(temporary.path()).expect("remote");
        assert!(matches!(
            remote.read("нет/такого.md"),
            Err(RemoteError::NotFound(_))
        ));
        assert!(remote.list("нет/такого").expect("list").is_empty());
        remote
            .delete("нет/такого.md")
            .expect("delete is idempotent");
    }

    #[test]
    fn directory_remote_rejects_empty_root() {
        assert!(DirectoryRemote::new("").is_err());
    }

    #[test]
    fn writes_are_atomic_via_temporary_file() {
        let temporary = tempfile::tempdir().expect("tempdir");
        let remote = DirectoryRemote::new(temporary.path()).expect("remote");
        remote.write("a.md", b"first").expect("write");
        remote.write("a.md", b"second").expect("rewrite");
        assert_eq!(remote.read("a.md").expect("read"), b"second");
        assert!(
            !temporary.path().join("a.part").exists(),
            "temporary file must be gone"
        );
    }
}
