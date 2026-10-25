//! Единый тип ошибки ядра.
//!
//! Все публичные функции возвращают [`Result<T>`] с [`Error`]. Варианты
//! намеренно «человеческие»: обёртка Tauri сериализует их в структуру
//! `{ kind, message, field }`, а фронтенд по `kind` показывает локализованный
//! текст и подсвечивает проблемное поле формы.

use thiserror::Error;

/// Ошибка ядра Bookmarks.
#[derive(Debug, Error)]
pub enum Error {
    /// Нарушение ограничений целостности или уникальности в БД.
    #[error("constraint violated: {0}")]
    Constraint(String),

    /// Запрошенная сущность не найдена.
    #[error("{entity} not found: {id}")]
    NotFound {
        /// Человекочитаемое имя сущности (`note`, `folder`, `tag`).
        entity: &'static str,
        /// Идентификатор, который не удалось найти.
        id: String,
    },

    /// Некорректные входные данные (пустое имя, недопустимый цвет, цикл в дереве папок).
    #[error("invalid input: {message}")]
    InvalidInput {
        /// Что именно не так — показывается пользователю.
        message: String,
        /// Поле формы, к которому относится ошибка (для подсветки).
        field: Option<String>,
    },

    /// Ошибка доступа к хранилищу.
    #[error("storage error: {0}")]
    Storage(#[from] rusqlite::Error),

    /// Ошибка миграции схемы.
    #[error("migration error: {0}")]
    Migration(String),

    /// Ошибка сериализации/десериализации.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Ошибка ввода-вывода (файлы экспорта, хранилище синхронизации).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Сейф: неверный пароль, повреждённый контейнер или несовместимая версия.
    #[error("vault error: {message}")]
    Vault {
        /// Причина — без секретов и без деталей, раскрывающих содержимое.
        message: String,
        /// Машиночитаемый код (`bad_password`, `bad_format`, `unsupported_version`).
        code: VaultErrorCode,
    },

    /// Синхронизация: транспорт недоступен или удалённое состояние несовместимо.
    #[error("sync error: {message}")]
    Sync {
        /// Причина сбоя.
        message: String,
        /// Машиночитаемый код (`offline`, `auth`, `conflict`, `remote_format`).
        code: SyncErrorCode,
    },
}

/// Код ошибки сейфа — для точной реакции UI (переспросить пароль vs. показать «файл повреждён»).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultErrorCode {
    /// Пароль не подошёл к верификатору.
    BadPassword,
    /// Контейнер не является файлом Bookmarks или повреждён.
    BadFormat,
    /// Версия контейнера новее поддерживаемой.
    UnsupportedVersion,
    /// Сейф ещё не инициализирован.
    NotInitialized,
}

/// Код ошибки синхронизации.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncErrorCode {
    /// Сеть/удалённый сервер недоступны.
    Offline,
    /// Проблема аутентификации (401/403, истёк токен).
    Auth,
    /// Конфликт версий, который не удалось разрешить автоматически.
    Conflict,
    /// Удалённое хранилище содержит данные чужого формата или более новой версии.
    RemoteFormat,
    /// Локальная ошибка при применении изменений.
    Local,
}

/// Результат с ошибкой ядра.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Конструктор ошибки ввода с указанием поля формы.
    pub fn invalid_field(message: impl Into<String>, field: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: message.into(),
            field: Some(field.into()),
        }
    }

    /// Конструктор ошибки ввода без привязки к полю.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: message.into(),
            field: None,
        }
    }

    /// Конструктор [`Error::NotFound`].
    pub fn not_found(entity: &'static str, id: impl Into<String>) -> Self {
        Self::NotFound {
            entity,
            id: id.into(),
        }
    }

    /// Конструктор ошибки сейфа.
    pub fn vault(message: impl Into<String>, code: VaultErrorCode) -> Self {
        Self::Vault {
            message: message.into(),
            code,
        }
    }

    /// Конструктор ошибки синхронизации.
    pub fn sync(message: impl Into<String>, code: SyncErrorCode) -> Self {
        Self::Sync {
            message: message.into(),
            code,
        }
    }

    /// Машиночитаемый код ошибки для IPC (`kind`), стабилен между версиями.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Constraint(_) => "constraint",
            Self::NotFound { .. } => "not_found",
            Self::InvalidInput { .. } => "invalid_input",
            Self::Storage(_) => "storage",
            Self::Migration(_) => "migration",
            Self::Serialization(_) => "serialization",
            Self::Io(_) => "io",
            Self::Vault { .. } => "vault",
            Self::Sync { .. } => "sync",
        }
    }
}

/// Представление ошибки, пригодное для передачи через IPC.
///
/// `field` нужен фронтенду, чтобы подсветить конкретное поле формы,
/// а не показывать общий тост.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ErrorPayload {
    /// Машиночитаемый код (см. [`Error::kind`]).
    pub kind: &'static str,
    /// Сообщение, пригодное для журнала.
    pub message: String,
    /// Поле формы, если ошибка к нему привязана.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// Дополнительный код для ошибок сейфа/синхронизации.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl From<Error> for ErrorPayload {
    fn from(error: Error) -> Self {
        let code = match &error {
            Error::Vault { code, .. } => Some(match code {
                VaultErrorCode::BadPassword => "bad_password",
                VaultErrorCode::BadFormat => "bad_format",
                VaultErrorCode::UnsupportedVersion => "unsupported_version",
                VaultErrorCode::NotInitialized => "not_initialized",
            }),
            Error::Sync { code, .. } => Some(match code {
                SyncErrorCode::Offline => "offline",
                SyncErrorCode::Auth => "auth",
                SyncErrorCode::Conflict => "conflict",
                SyncErrorCode::RemoteFormat => "remote_format",
                SyncErrorCode::Local => "local",
            }),
            _ => None,
        };
        let field = match &error {
            Error::InvalidInput { field, .. } => field.clone(),
            _ => None,
        };
        Self {
            kind: error.kind(),
            message: error.to_string(),
            field,
            code: code.map(str::to_owned),
        }
    }
}
