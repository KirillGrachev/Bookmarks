//! Пользовательские функции SQLite.
//!
//! Триггеры поискового индекса должны класть в FTS5 текст без Markdown-разметки
//! (иначе поиск находил бы заметки по звёздочкам и скобкам), а выразить это
//! средствами SQL нельзя. Поэтому извлечение текста зарегистрировано как
//! скалярная функция `mb_plain_text` и используется прямо в триггерах.
//!
//! Функция объявлена детерминированной: при одинаковом входе результат
//! одинаков, а значит планировщик может кэшировать вычисления.

use rusqlite::functions::FunctionFlags;
use rusqlite::{types::ValueRef, Connection};

use crate::error::Result;

/// Имя функции в SQL.
pub const PLAIN_TEXT_FUNCTION: &str = "mb_plain_text";

/// Регистрирует пользовательские функции на соединении.
///
/// Вызывается до применения миграций: `CREATE TRIGGER` не выполняется, если
/// функция в его теле неизвестна.
pub fn register(connection: &Connection) -> Result<()> {
    connection.create_scalar_function(
        PLAIN_TEXT_FUNCTION,
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |context| {
            // NULL и не-текст дают NULL: функция не должна паниковать на
            // повреждённых данных, а поиск по NULL просто ничего не находит.
            let text = match context.get_raw(0) {
                ValueRef::Text(value) => String::from_utf8_lossy(value).into_owned(),
                _ => return Ok(None as Option<String>),
            };
            Ok(Some(crate::search::plaintext(&text)))
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::*;

    #[test]
    fn strips_markdown_inside_sql() {
        let connection = Connection::open_in_memory().expect("db");
        register(&connection).expect("register");
        let text: String = connection
            .query_row(
                "SELECT mb_plain_text('# Заголовок **жирный**')",
                [],
                |row| row.get(0),
            )
            .expect("call");
        assert_eq!(text, "Заголовок жирный");

        let null: Option<String> = connection
            .query_row("SELECT mb_plain_text(NULL)", [], |row| row.get(0))
            .expect("call");
        assert!(null.is_none());
    }

    #[test]
    fn is_deterministic_and_cacheable() {
        let connection = Connection::open_in_memory().expect("db");
        register(&connection).expect("register");
        let first: String = connection
            .query_row("SELECT mb_plain_text('*a*')", [], |row| row.get(0))
            .expect("call");
        let second: String = connection
            .query_row("SELECT mb_plain_text('*a*')", [], |row| row.get(0))
            .expect("call");
        assert_eq!(first, second);
        assert_eq!(first, "a");
    }
}
