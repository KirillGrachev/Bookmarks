//! Идентификаторы сущностей.
//!
//! Внутренние первичные ключи в SQLite — `INTEGER` (быстрые соединения и
//! компактные индексы), а наружу, в IPC и в файлы синхронизации, отдаются
//! строковые UUID v4: они стабильны между устройствами и не раскрывают
//! порядок создания записей.

use uuid::Uuid;

/// Публичный идентификатор сущности (UUID v4 в канонической форме).
pub type EntityId = String;

/// Создаёт новый идентификатор.
pub fn new_id() -> EntityId {
    Uuid::new_v4().to_string()
}

/// Проверяет, что строка является каноническим UUID.
///
/// Используется при импорте и при применении удалённых изменений: чужой
/// идентификатор произвольной формы в базу не пускаем.
pub fn is_valid_id(value: &str) -> bool {
    Uuid::parse_str(value).is_ok() && value.len() == 36
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_canonical_uuid() {
        let id = new_id();
        assert_eq!(id.len(), 36);
        assert!(is_valid_id(&id));
        assert_ne!(id, new_id());
    }

    #[test]
    fn rejects_foreign_identifiers() {
        assert!(!is_valid_id(""));
        assert!(!is_valid_id("42"));
        assert!(!is_valid_id("not-a-uuid-at-all-nope-nope-nope-nope-1"));
        // UUID без дефисов валиден для парсера, но не является канонической формой.
        assert!(!is_valid_id("67e5504410b1426f9247bb680e5fe0c8"));
        assert!(is_valid_id("67e55044-10b1-426f-9247-bb680e5fe0c8"));
    }
}
