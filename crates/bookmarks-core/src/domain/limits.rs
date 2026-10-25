//! Ограничения доменных значений.
//!
//! Лимиты нужны не для «защиты от пользователя», а чтобы приложение оставалось
//! отзывчивым: заголовок в 10 000 символов ломает вёрстку списка, а тело
//! заметки в сотни мегабайт — полнотекстовый индекс. Значения подобраны с
//! запасом: обычная заметка в десятки раз меньше.

/// Максимальная длина заголовка заметки, в символах.
pub const MAX_TITLE_CHARS: usize = 300;

/// Максимальная длина тела заметки (Markdown), в символах.
pub const MAX_BODY_CHARS: usize = 1_000_000;

/// Максимальная длина названия папки, в символах.
pub const MAX_FOLDER_NAME_CHARS: usize = 120;

/// Максимальная длина названия тега, в символах.
pub const MAX_TAG_NAME_CHARS: usize = 60;

/// Максимальная глубина вложенности папок.
pub const MAX_FOLDER_DEPTH: u32 = 16;

/// Максимальное число тегов у одной заметки.
pub const MAX_TAGS_PER_NOTE: usize = 50;

/// Максимальное число сохранённых версий одной заметки.
pub const MAX_VERSIONS_PER_NOTE: usize = 200;

/// Максимальный размер одного вложения, в байтах (64 МиБ).
pub const MAX_ATTACHMENT_BYTES: u64 = 64 * 1024 * 1024;

/// Допустимые цвета метки заметки/папки/тега (hex без `#`, нижний регистр).
///
/// Палитра совпадает с акцентами интерфейса, поэтому список закрытый:
/// произвольный цвет из пипетки мог бы оказаться нечитаемым в тёмной теме.
pub const ALLOWED_COLORS: [&str; 9] = [
    "slate", "blue", "cyan", "green", "amber", "orange", "red", "pink", "violet",
];

/// Проверяет, что строка является допустимым именем цвета.
pub fn is_allowed_color(value: &str) -> bool {
    ALLOWED_COLORS.contains(&value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_whitelist_works() {
        assert!(is_allowed_color("blue"));
        assert!(is_allowed_color("slate"));
        assert!(!is_allowed_color("#2563eb"));
        assert!(!is_allowed_color(""));
        assert!(!is_allowed_color("BLUE"));
    }

    #[test]
    fn palette_covers_interface_accents() {
        // Палитра меток должна покрывать акценты интерфейса, иначе метку
        // нельзя было бы подобрать в тон выбранному цвету приложения.
        let colors = ALLOWED_COLORS.map(str::to_owned);
        assert!(colors.contains(&"blue".to_owned()));
        assert!(colors.contains(&"slate".to_owned()));
        assert!(!colors.contains(&"chartreuse".to_owned()));
        assert!(colors
            .iter()
            .all(|color| !color.is_empty() && color.chars().all(|c| c.is_ascii_lowercase())));
    }
}
