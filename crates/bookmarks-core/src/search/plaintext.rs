//! Извлечение обычного текста из Markdown.
//!
//! Один и тот же результат используется для трёх задач: поискового индекса
//! (чтобы `**важное**` искалось как «важное»), превью в списке и экспорта в
//! HTML-описание. Реализация на `pulldown-cmark` без аллокаций DOM: событие
//! `Text` — это уже «человеческий» текст без разметки.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Опции парсера: включаем всё, что встречается в реальных заметках.
fn parser_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_SMART_PUNCTUATION
        | Options::ENABLE_HEADING_ATTRIBUTES
}

/// Преобразует Markdown в текст без разметки.
///
/// Правила:
/// - блоки кода попадают в текст целиком (поиск по коду работает);
/// - у ссылок остаётся только текст подписи (`[документ](http://…)` → «документ»);
/// - заголовки, пункты списков и ячейки таблиц разделяются переводами строк
///   или пробелами — индекс FTS5 всё равно токенизирует по пробелам;
/// - HTML-теги отбрасываются (в индекс не должны попадать `<div>` и подобное).
pub fn plaintext(markdown: &str) -> String {
    if markdown.is_empty() {
        return String::new();
    }
    let parser = Parser::new_ext(markdown, parser_options());
    let mut output = String::with_capacity(markdown.len());
    let mut in_code_block = false;

    for event in parser {
        match event {
            Event::Text(text) => {
                output.push_str(&text);
                if in_code_block {
                    output.push('\n');
                }
            }
            Event::Code(code) => {
                output.push(' ');
                output.push_str(&code);
                output.push(' ');
            }
            Event::SoftBreak | Event::HardBreak => output.push('\n'),
            Event::Start(Tag::CodeBlock(_)) => {
                in_code_block = true;
                output.push('\n');
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                output.push('\n');
            }
            Event::Start(Tag::Heading { level, .. }) => {
                output.push('\n');
                // Маркеры уровня заголовка в текст не попадают, но разделение
                // строк важно, чтобы «Итоги» и «2026» не склеились в одно слово.
                let _ = level;
            }
            Event::End(TagEnd::Heading(_)) => output.push('\n'),
            Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph) => output.push('\n'),
            Event::Start(Tag::Item) => output.push_str("\n- "),
            Event::End(TagEnd::Item) => output.push('\n'),
            Event::Start(Tag::List(_)) | Event::End(TagEnd::List(_)) => output.push('\n'),
            Event::Start(Tag::Table(_)) | Event::End(TagEnd::Table) => output.push('\n'),
            Event::Start(Tag::TableRow) | Event::End(TagEnd::TableRow) => output.push('\n'),
            Event::Start(Tag::TableCell) | Event::End(TagEnd::TableCell) => output.push(' '),
            Event::Start(Tag::BlockQuote(_)) | Event::End(TagEnd::BlockQuote(_)) => {
                output.push('\n')
            }
            Event::Rule => output.push('\n'),
            Event::Html(_) | Event::InlineHtml(_) => {}
            Event::FootnoteReference(reference) => {
                output.push(' ');
                output.push_str(&reference);
            }
            _ => {}
        }
    }

    normalize_whitespace(&output)
}

/// Схлопывает повторяющиеся пробелы и пустые строки.
///
/// Индексу и превью не нужны десятки переводов строк из исходной разметки:
/// после нормализации позиция символа в тексте предсказуема, а `snippet()`
/// FTS5 выдаёт компактный фрагмент.
pub fn normalize_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut pending_spaces = 0usize;
    let mut pending_newlines = 0usize;

    for character in text.chars() {
        if character == '\n' {
            pending_newlines += 1;
            continue;
        }
        if character.is_whitespace() {
            pending_spaces += 1;
            continue;
        }
        if pending_newlines > 1 {
            // Пустая строка разделяет блоки: сохраняем один перевод, чтобы
            // структура заметки оставалась различимой в превью.
            result.push('\n');
        } else if (pending_newlines == 1 || pending_spaces > 0) && !result.is_empty() {
            result.push(' ');
        }
        pending_newlines = 0;
        pending_spaces = 0;
        result.push(character);
    }
    result.trim().to_owned()
}

/// Есть ли в документе изображения.
pub fn contains_image(markdown: &str) -> bool {
    Parser::new_ext(markdown, parser_options()).any(|event| {
        matches!(
            event,
            Event::Start(Tag::Image { .. }) | Event::End(TagEnd::Image)
        )
    })
}

/// Есть ли в документе ссылки.
pub fn contains_link(markdown: &str) -> bool {
    Parser::new_ext(markdown, parser_options())
        .any(|event| matches!(event, Event::Start(Tag::Link { .. })))
}

/// Есть ли блоки или инлайновые фрагменты кода.
pub fn contains_code(markdown: &str) -> bool {
    Parser::new_ext(markdown, parser_options())
        .any(|event| matches!(event, Event::Start(Tag::CodeBlock(_)) | Event::Code(_)))
}

/// Есть ли пункты списка задач (`- [ ]` / `- [x]`).
pub fn contains_task(markdown: &str) -> bool {
    Parser::new_ext(markdown, parser_options()).any(|event| match event {
        Event::TaskListMarker(_) => true,
        Event::Text(text) => text.contains("[ ]") || text.contains("[x]"),
        _ => false,
    })
}

/// Есть ли таблицы.
pub fn contains_table(markdown: &str) -> bool {
    Parser::new_ext(markdown, parser_options())
        .any(|event| matches!(event, Event::Start(Tag::Table(_))))
}

/// Есть ли ссылки на вложения (`attachment:…` или локальные пути `./assets/…`).
pub fn contains_attachment(markdown: &str) -> bool {
    Parser::new_ext(markdown, parser_options()).any(|event| match event {
        Event::Start(Tag::Link { dest_url, .. }) | Event::Start(Tag::Image { dest_url, .. }) => {
            dest_url.starts_with("attachment:")
                || dest_url.starts_with("./assets/")
                || dest_url.starts_with("assets/")
                || dest_url.starts_with("file:")
        }
        _ => false,
    })
}

/// Заголовок первого уровня, если он есть (для автоматического названия заметки).
pub fn first_heading(markdown: &str) -> Option<String> {
    let parser = Parser::new_ext(markdown, parser_options());
    let mut inside_heading: Option<HeadingLevel> = None;
    let mut collected = String::new();
    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => inside_heading = Some(level),
            Event::End(TagEnd::Heading(_)) => {
                if inside_heading == Some(HeadingLevel::H1) {
                    let title = collected.trim().to_owned();
                    if !title.is_empty() {
                        return Some(title);
                    }
                }
                inside_heading = None;
                collected.clear();
            }
            Event::Text(text) if inside_heading.is_some() => collected.push_str(&text),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_inline_markup() {
        assert_eq!(plaintext("**жирный** и *курсив*"), "жирный и курсив");
        assert_eq!(plaintext("`код` внутри"), "код внутри");
        assert_eq!(plaintext("~~зачёркнуто~~"), "зачёркнуто");
    }

    #[test]
    fn keeps_link_text_and_drops_url() {
        assert_eq!(
            plaintext("[документ](https://example.com/file.pdf)"),
            "документ"
        );
        assert_eq!(
            plaintext("Смотри [документ](https://example.com) и ![скрин](img/a.png)"),
            "Смотри документ и скрин"
        );
    }

    #[test]
    fn keeps_code_block_content() {
        let markdown = "Текст\n\n```rust\nfn main() {}\n```\n\nКонец";
        let text = plaintext(markdown);
        assert!(
            text.contains("fn main"),
            "code must stay searchable: {text}"
        );
        assert!(text.contains("Текст"));
        assert!(text.contains("Конец"));
        assert!(!text.contains("```"));
    }

    #[test]
    fn separates_headings_and_items() {
        let text = plaintext("# Итоги\n- молоко\n- хлеб\n## 2026");
        assert!(text.contains("Итоги"));
        assert!(text.contains("молоко"));
        assert!(text.contains("2026"));
        assert!(!text.contains("#"));
        // Слова из разных строк не склеиваются.
        assert!(!text.contains("Итогими"));
    }

    #[test]
    fn drops_html_markup() {
        // Блочный HTML парсер отдаёт целиком как `Event::Html` — в индекс он
        // не попадает (иначе поиск находил бы заметки по именам тегов).
        assert_eq!(plaintext("<div class='x'>текст</div>"), "");
        // Инлайновый HTML снимается, текст вокруг сохраняется.
        assert_eq!(plaintext("до <b>жирный</b> после"), "до жирный после");
        assert!(!plaintext("текст <br> ещё").contains('<'));
    }

    #[test]
    fn normalizes_whitespace() {
        assert_eq!(normalize_whitespace("a   b\n\n\n c "), "a b\nc");
        assert_eq!(normalize_whitespace("   "), "");
        assert_eq!(normalize_whitespace(""), "");
    }

    #[test]
    fn detects_content_kinds() {
        let rich = "Задача:\n- [ ] купить\n\n![img](attachment:abc)\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```js\nx=1\n```\n\n[ссылка](https://e.com)";
        assert!(contains_image(rich));
        assert!(contains_link(rich));
        assert!(contains_code(rich));
        assert!(contains_task(rich));
        assert!(contains_table(rich));
        assert!(contains_attachment(rich));

        let plain = "Просто текст без всего";
        assert!(!contains_image(plain));
        assert!(!contains_link(plain));
        assert!(!contains_code(plain));
        assert!(!contains_task(plain));
        assert!(!contains_table(plain));
        assert!(!contains_attachment(plain));
    }

    #[test]
    fn finds_first_h1() {
        assert_eq!(
            first_heading("# Заголовок\nтекст").as_deref(),
            Some("Заголовок")
        );
        assert_eq!(
            first_heading("## Не он\n# Вот он").as_deref(),
            Some("Вот он")
        );
        assert_eq!(first_heading("без заголовков"), None);
        assert_eq!(first_heading("#   "), None);
    }

    #[test]
    fn handles_empty_and_huge_input() {
        assert_eq!(plaintext(""), "");
        let big = "слово ".repeat(10_000);
        assert!(plaintext(&big).len() > 10_000);
    }
}
