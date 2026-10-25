//! Экспорт в единый HTML-файл.
//!
//! Один самодостаточный документ: стили встроены, внешних ресурсов нет,
//! поэтому файл открывается офлайн, печатается и пересылается коллегам без
//! приложения. Палитра повторяет токены интерфейса, так что выгрузка выглядит
//! как само Bookmarks.
//!
//! Markdown преобразуется собственным минимальным рендерером: весь текст
//! экранируется, поэтому HTML из тела заметки не исполняется (экспорт не должен
//! становиться каналом для чужого скрипта).

use crate::export::{ExportNote, ExportOptions};
use crate::time::Timestamp;

/// Имя файла экспорта.
pub const DEFAULT_FILE_NAME: &str = "bookmarks-export.html";

/// Рендерит библиотеку в HTML-документ.
pub fn render(notes: &[ExportNote], options: &ExportOptions) -> crate::export::ExportedFile {
    let mut body = String::new();
    body.push_str("<div class=\"layout\">\n");
    body.push_str(&table_of_contents(notes));
    body.push_str("<main class=\"content\">\n");
    for (index, note) in notes.iter().enumerate() {
        body.push_str(&article(note, index, options));
    }
    if notes.is_empty() {
        body.push_str("<p class=\"empty\">В выгрузке нет заметок.</p>\n");
    }
    body.push_str("</main>\n</div>\n");

    let html = format!(
        "<!DOCTYPE html>\n<html lang=\"ru\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title}</title>\n<style>\n{styles}\n</style>\n</head>\n<body>\n\
         <header class=\"page-header\">\n<h1>{title}</h1>\n<p class=\"meta\">{subtitle}</p>\n</header>\n\
         {body}\n</body>\n</html>\n",
        title = escape_html("Bookmarks — выгрузка заметок"),
        subtitle = subtitle(notes),
        styles = STYLES,
        body = body
    );

    crate::export::ExportedFile {
        path: DEFAULT_FILE_NAME.to_owned(),
        content: html,
    }
}

fn subtitle(notes: &[ExportNote]) -> String {
    format!(
        "Заметок: {} · выгружено {}",
        notes.len(),
        format_timestamp(Timestamp::now())
    )
}

fn table_of_contents(notes: &[ExportNote]) -> String {
    let mut out = String::from("<nav class=\"toc\">\n<h2>Содержание</h2>\n");
    let mut current_folder = String::from("\u{0}");
    for (index, note) in notes.iter().enumerate() {
        if note.folder_path != current_folder {
            if current_folder != "\u{0}" {
                out.push_str("</ul>\n");
            }
            current_folder.clone_from(&note.folder_path);
            let heading = if current_folder.is_empty() {
                "Без папки"
            } else {
                current_folder.as_str()
            };
            out.push_str(&format!("<h3>{}</h3>\n<ul>\n", escape_html(heading)));
        }
        let title = if note.title.trim().is_empty() {
            "Без названия"
        } else {
            note.title.as_str()
        };
        out.push_str(&format!(
            "<li><a href=\"#note-{index}\">{title}{badge}</a></li>\n",
            index = index,
            title = escape_html(title),
            badge = if note.encrypted { " 🔒" } else { "" }
        ));
    }
    if current_folder != "\u{0}" {
        out.push_str("</ul>\n");
    }
    out.push_str("</nav>\n");
    out
}

fn article(note: &ExportNote, index: usize, options: &ExportOptions) -> String {
    let title = if note.title.trim().is_empty() {
        "Без названия".to_owned()
    } else {
        note.title.clone()
    };
    let mut classes = String::from("note");
    if let Some(color) = &note.color {
        classes.push_str(&format!(" accent-{color}"));
    }

    let mut meta = vec![format_timestamp(note.updated_at)];
    if !note.folder_path.is_empty() {
        meta.push(note.folder_path.clone());
    }
    if note.pinned {
        meta.push("закреплена".to_owned());
    }
    if note.archived {
        meta.push("в архиве".to_owned());
    }
    if note.encrypted {
        meta.push("из сейфа".to_owned());
    }

    let tags = if note.tags.is_empty() {
        String::new()
    } else {
        format!(
            "<div class=\"tags\">{}</div>",
            note.tags
                .iter()
                .map(|tag| format!("<span class=\"tag\">#{}</span>", escape_html(tag)))
                .collect::<Vec<_>>()
                .join("")
        )
    };

    let body = if note.encrypted && !options.include_encrypted {
        "<p class=\"encrypted-note\">Заметка в сейфе: содержимое не выгружено.</p>".to_owned()
    } else {
        markdown_to_html(&note.body)
    };

    format!(
        "<article class=\"{classes}\" id=\"note-{index}\">\n\
         <h2 class=\"note-title\">{title}</h2>\n\
         <p class=\"note-meta\">{meta}</p>\n{tags}\n\
         <div class=\"note-body\">\n{body}\n</div>\n</article>\n",
        classes = classes,
        index = index,
        title = escape_html(&title),
        meta = escape_html(&meta.join(" · ")),
        tags = tags,
        body = body
    )
}

/// Форматирует метку времени для документа (UTC, без секунд).
fn format_timestamp(timestamp: Timestamp) -> String {
    match timestamp.to_datetime() {
        Some(datetime) => datetime.format("%Y-%m-%d %H:%M").to_string(),
        None => "—".to_owned(),
    }
}

/// Экранирует текст для вставки в HTML.
pub fn escape_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

/// Минимальный рендерер Markdown в HTML.
///
/// Поддерживает то, что реально встречается в заметках: заголовки, абзацы,
/// списки (в том числе задачи), цитаты, блоки кода, таблицы, горизонтальные
/// линейки и инлайн-разметку. Всё, что не распознано, выводится как
/// экранированный текст.
pub fn markdown_to_html(markdown: &str) -> String {
    let mut out = String::new();
    let lines = markdown.lines();
    let mut paragraph: Vec<String> = Vec::new();
    let mut list_stack: Vec<bool> = Vec::new(); // true — нумерованный список
    let mut in_code = false;
    let mut code_buffer: Vec<String> = Vec::new();
    let mut table_buffer: Vec<String> = Vec::new();

    let flush_paragraph = |out: &mut String, paragraph: &mut Vec<String>| {
        if paragraph.is_empty() {
            return;
        }
        out.push_str("<p>");
        out.push_str(&inline(&paragraph.join(" ")));
        out.push_str("</p>\n");
        paragraph.clear();
    };
    let flush_lists = |out: &mut String, list_stack: &mut Vec<bool>| {
        while let Some(ordered) = list_stack.pop() {
            out.push_str(if ordered { "</ol>\n" } else { "</ul>\n" });
        }
    };
    let flush_table = |out: &mut String, table_buffer: &mut Vec<String>| {
        if table_buffer.is_empty() {
            return;
        }
        out.push_str(&render_table(table_buffer));
        table_buffer.clear();
    };

    for line in lines {
        if line.trim_start().starts_with("```") {
            if in_code {
                out.push_str(&format!(
                    "<pre><code>{}</code></pre>\n",
                    escape_html(&code_buffer.join("\n"))
                ));
                code_buffer.clear();
                in_code = false;
            } else {
                flush_paragraph(&mut out, &mut paragraph);
                flush_lists(&mut out, &mut list_stack);
                flush_table(&mut out, &mut table_buffer);
                in_code = true;
            }
            continue;
        }
        if in_code {
            code_buffer.push(line.to_owned());
            continue;
        }

        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
            flush_paragraph(&mut out, &mut paragraph);
            flush_lists(&mut out, &mut list_stack);
            flush_table(&mut out, &mut table_buffer);
            continue;
        }

        if is_table_row(trimmed) {
            flush_paragraph(&mut out, &mut paragraph);
            flush_lists(&mut out, &mut list_stack);
            table_buffer.push(trimmed.to_owned());
            continue;
        }
        flush_table(&mut out, &mut table_buffer);

        if let Some(level) = heading_level(trimmed) {
            flush_paragraph(&mut out, &mut paragraph);
            flush_lists(&mut out, &mut list_stack);
            let text = trimmed[level + 1..].trim();
            out.push_str(&format!(
                "<h{level}>{text}</h{level}>\n",
                level = level.min(6),
                text = inline(text)
            ));
            continue;
        }

        if is_rule(trimmed) {
            flush_paragraph(&mut out, &mut paragraph);
            flush_lists(&mut out, &mut list_stack);
            out.push_str("<hr>\n");
            continue;
        }

        if let Some(quote) = trimmed.strip_prefix('>') {
            flush_paragraph(&mut out, &mut paragraph);
            flush_lists(&mut out, &mut list_stack);
            out.push_str(&format!(
                "<blockquote><p>{}</p></blockquote>\n",
                inline(quote.trim())
            ));
            continue;
        }

        if let Some(item) = parse_list_item(trimmed) {
            flush_paragraph(&mut out, &mut paragraph);
            let expected = item.ordered;
            if list_stack.last() != Some(&expected) {
                flush_lists(&mut out, &mut list_stack);
                out.push_str(if expected { "<ol>\n" } else { "<ul>\n" });
                list_stack.push(expected);
            }
            let marker = match item.task {
                Some(done) => {
                    format!(
                        "<input type=\"checkbox\" disabled{}> ",
                        if done { " checked" } else { "" }
                    )
                }
                None => String::new(),
            };
            out.push_str(&format!("<li>{marker}{}</li>\n", inline(item.text)));
            continue;
        }

        flush_lists(&mut out, &mut list_stack);
        paragraph.push(trimmed.to_owned());
    }

    if in_code && !code_buffer.is_empty() {
        out.push_str(&format!(
            "<pre><code>{}</code></pre>\n",
            escape_html(&code_buffer.join("\n"))
        ));
    }
    flush_paragraph(&mut out, &mut paragraph);
    flush_lists(&mut out, &mut list_stack);
    flush_table(&mut out, &mut table_buffer);
    out
}

/// Пункт списка.
struct ListItem<'a> {
    ordered: bool,
    text: &'a str,
    task: Option<bool>,
}

fn parse_list_item(line: &str) -> Option<ListItem<'_>> {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        let task = match rest.strip_prefix("[ ] ") {
            Some(text) => {
                return Some(ListItem {
                    ordered: false,
                    text,
                    task: Some(false),
                })
            }
            None => match rest
                .strip_prefix("[x] ")
                .or_else(|| rest.strip_prefix("[X] "))
            {
                Some(text) => {
                    return Some(ListItem {
                        ordered: false,
                        text,
                        task: Some(true),
                    })
                }
                None => None,
            },
        };
        return Some(ListItem {
            ordered: false,
            text: rest,
            task,
        });
    }
    let digits_end = trimmed.find(|character: char| !character.is_ascii_digit())?;
    if digits_end == 0 || digits_end > 9 {
        return None;
    }
    let rest = &trimmed[digits_end..];
    let text = rest
        .strip_prefix(". ")
        .or_else(|| rest.strip_prefix(") "))?;
    Some(ListItem {
        ordered: true,
        text,
        task: None,
    })
}

fn heading_level(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }
    let level = trimmed
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if level > 6 || !trimmed[level..].starts_with(' ') {
        return None;
    }
    Some(level)
}

fn is_rule(line: &str) -> bool {
    let trimmed = line.trim();
    let without_spaces: String = trimmed
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    without_spaces.len() >= 3
        && (without_spaces.chars().all(|character| character == '-')
            || without_spaces.chars().all(|character| character == '*')
            || without_spaces.chars().all(|character| character == '_'))
}

fn is_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() > 2
}

fn render_table(rows: &[String]) -> String {
    let mut out = String::from("<table>\n");
    for (index, row) in rows.iter().enumerate() {
        let cells: Vec<&str> = row
            .trim()
            .trim_matches('|')
            .split('|')
            .map(|cell| cell.trim())
            .collect();
        // Строка-разделитель (`|---|---|`) не выводится.
        if cells.iter().all(|cell| {
            cell.chars()
                .all(|character| matches!(character, '-' | ':' | ' '))
                && !cell.is_empty()
        }) {
            continue;
        }
        let tag = if index == 0 { "th" } else { "td" };
        out.push_str("<tr>");
        for cell in cells {
            out.push_str(&format!("<{tag}>{}</{tag}>", inline(cell)));
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</table>\n");
    out
}

/// Инлайн-разметка: код, изображения, ссылки, жирный, курсив, зачёркнутый.
///
/// Рекурсивный спуск по исходному тексту: каждый найденный маркер обрабатывает
/// своё содержимое отдельно, поэтому `**жирный со [ссылкой](url)**` работает, а
/// вложенность не «съедает» разметку. Всё, что не распознано, экранируется —
/// HTML из тела заметки не может исполниться.
pub fn inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;

    while let Some(position) = next_marker(rest) {
        let (marker, start, length) = position;
        out.push_str(&escape_html(&rest[..start]));
        let after_marker = &rest[start + length..];
        match marker {
            Marker::Code => match find_close(after_marker, "`") {
                Some((inner, tail)) => {
                    out.push_str("<code>");
                    out.push_str(&escape_html(inner));
                    out.push_str("</code>");
                    rest = tail;
                }
                None => {
                    out.push('`');
                    rest = after_marker;
                }
            },
            Marker::Image => match split_link(after_marker) {
                Some((label, url, tail)) => {
                    out.push_str(&format!(
                        "<img src=\"{}\" alt=\"{}\">",
                        escape_attribute(url),
                        escape_attribute(label)
                    ));
                    rest = tail;
                }
                None => {
                    out.push_str(&escape_html(&rest[start..start + length]));
                    rest = after_marker;
                }
            },
            Marker::Link => match split_link(after_marker) {
                Some((label, url, tail)) => {
                    out.push_str(&format!(
                        "<a href=\"{}\">{}</a>",
                        escape_attribute(url),
                        inline(label)
                    ));
                    rest = tail;
                }
                None => {
                    out.push_str(&escape_html(&rest[start..start + length]));
                    rest = after_marker;
                }
            },
            Marker::Strong => {
                match find_close(after_marker, "**").or_else(|| find_close(after_marker, "__")) {
                    Some((inner, tail)) => {
                        out.push_str("<strong>");
                        out.push_str(&inline(inner));
                        out.push_str("</strong>");
                        rest = tail;
                    }
                    None => {
                        out.push_str(&escape_html(&rest[start..start + length]));
                        rest = after_marker;
                    }
                }
            }
            Marker::Strike => match find_close(after_marker, "~~") {
                Some((inner, tail)) => {
                    out.push_str("<del>");
                    out.push_str(&inline(inner));
                    out.push_str("</del>");
                    rest = tail;
                }
                None => {
                    out.push_str(&escape_html(&rest[start..start + length]));
                    rest = after_marker;
                }
            },
            Marker::Emphasis => {
                match find_close(after_marker, "*").or_else(|| find_close(after_marker, "_")) {
                    Some((inner, tail)) if !inner.is_empty() => {
                        out.push_str("<em>");
                        out.push_str(&inline(inner));
                        out.push_str("</em>");
                        rest = tail;
                    }
                    _ => {
                        out.push_str(&escape_html(&rest[start..start + length]));
                        rest = after_marker;
                    }
                }
            }
        }
    }
    out.push_str(&escape_html(rest));
    out
}

/// Маркеры инлайн-разметки в порядке приоритета.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Marker {
    Code,
    Image,
    Link,
    Strong,
    Strike,
    Emphasis,
}

impl Marker {
    fn as_str(self) -> &'static str {
        match self {
            Self::Code => "`",
            Self::Image => "![",
            Self::Link => "[",
            Self::Strong => "**",
            Self::Strike => "~~",
            Self::Emphasis => "*",
        }
    }
}

/// Позиция ближайшего маркера: (маркер, смещение, длина).
fn next_marker(text: &str) -> Option<(Marker, usize, usize)> {
    // Порядок важен: `**` проверяется раньше `*`, `![` раньше `[`.
    const ORDER: [(Marker, &str); 6] = [
        (Marker::Image, "!["),
        (Marker::Code, "`"),
        (Marker::Strong, "**"),
        (Marker::Strike, "~~"),
        (Marker::Link, "["),
        (Marker::Emphasis, "*"),
    ];
    let mut best: Option<(Marker, usize, usize)> = None;
    for (marker, pattern) in ORDER {
        if let Some(found) = text.find(pattern) {
            let candidate = (marker, found, pattern.len());
            match best {
                Some((_, best_start, _)) if best_start <= found => {}
                _ => best = Some(candidate),
            }
        }
    }
    best.map(|(marker, start, _)| (marker, start, marker.as_str().len()))
}

/// Ищет закрывающий маркер; возвращает содержимое и остаток строки.
fn find_close<'text>(text: &'text str, close: &str) -> Option<(&'text str, &'text str)> {
    let end = text.find(close)?;
    if end == 0 {
        return None;
    }
    Some((&text[..end], &text[end + close.len()..]))
}

/// Разбирает `подпись](адрес)` — общую часть ссылок и изображений.
fn split_link(text: &str) -> Option<(&str, &str, &str)> {
    let bracket_end = text.find(']')?;
    let label = &text[..bracket_end];
    let after_label = &text[bracket_end + 1..];
    let url_part = after_label.strip_prefix('(')?;
    let paren_end = url_part.find(')')?;
    let url = url_part[..paren_end].trim();
    if url.is_empty() {
        return None;
    }
    Some((label, url, &url_part[paren_end + 1..]))
}

/// Экранирует значение атрибута и отсекает опасные схемы.
///
/// `javascript:` и `data:` в href — классический способ превратить выгрузку в
/// исполняемый файл, поэтому такие адреса заменяются пустой ссылкой.
fn escape_attribute(url: &str) -> String {
    let trimmed = url.trim();
    let lowered = trimmed.to_ascii_lowercase();
    let dangerous = lowered.starts_with("javascript:")
        || lowered.starts_with("vbscript:")
        || (lowered.starts_with("data:") && !lowered.starts_with("data:image/"));
    let safe = if dangerous { "#" } else { trimmed };
    safe.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Встроенные стили документа — палитра приложения.
const STYLES: &str = r#":root {
  color-scheme: light dark;
  --background: #ffffff;
  --foreground: #334155;
  --surface: #ffffff;
  --muted: #f1f5f9;
  --muted-foreground: #64748b;
  --border: #e2e8f0;
  --primary: #2563eb;
}
@media (prefers-color-scheme: dark) {
  :root {
    --background: #09090b;
    --foreground: #fafafa;
    --surface: #09090b;
    --muted: #0e0e10;
    --muted-foreground: #a1a1aa;
    --border: rgba(255, 255, 255, 0.1);
    --primary: #3b82f6;
  }
}
* { box-sizing: border-box; }
body {
  margin: 0;
  background: var(--background);
  color: var(--foreground);
  font: 15px/1.65 ui-sans-serif, system-ui, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
}
.page-header {
  padding: 28px 32px 16px;
  border-bottom: 1px solid var(--border);
}
.page-header h1 { margin: 0; font-size: 20px; letter-spacing: -0.01em; }
.page-header .meta { margin: 6px 0 0; color: var(--muted-foreground); font-size: 13px; }
.layout { display: grid; grid-template-columns: 260px minmax(0, 1fr); gap: 24px; padding: 24px 32px 64px; }
.toc { position: sticky; top: 24px; align-self: start; max-height: calc(100vh - 48px); overflow: auto; }
.toc h2 { font-size: 12px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--muted-foreground); margin: 0 0 8px; }
.toc h3 { font-size: 13px; margin: 16px 0 6px; }
.toc ul { list-style: none; margin: 0; padding: 0; }
.toc li { margin: 2px 0; }
.toc a { color: var(--foreground); text-decoration: none; font-size: 13px; display: block; padding: 3px 8px; border-radius: 6px; }
.toc a:hover { background: var(--muted); }
.note {
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 20px 22px;
  margin-bottom: 18px;
  background: var(--surface);
  break-inside: avoid;
}
.note-title { margin: 0 0 4px; font-size: 17px; letter-spacing: -0.01em; }
.note-meta { margin: 0 0 10px; color: var(--muted-foreground); font-size: 12px; }
.note-body h1, .note-body h2, .note-body h3 { margin: 18px 0 8px; font-size: 15px; }
.note-body p { margin: 8px 0; }
.note-body ul, .note-body ol { margin: 8px 0; padding-left: 22px; }
.note-body blockquote { margin: 10px 0; padding: 6px 14px; border-left: 3px solid var(--primary); color: var(--muted-foreground); }
.note-body pre { background: var(--muted); padding: 12px 14px; border-radius: 8px; overflow: auto; }
.note-body code { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 13px; }
.note-body table { border-collapse: collapse; margin: 12px 0; }
.note-body th, .note-body td { border: 1px solid var(--border); padding: 6px 10px; font-size: 13px; }
.note-body hr { border: none; border-top: 1px solid var(--border); margin: 16px 0; }
.tags { margin-bottom: 10px; }
.tag {
  display: inline-block; margin-right: 6px; padding: 2px 8px; border-radius: 999px;
  background: var(--muted); color: var(--muted-foreground); font-size: 12px;
}
.encrypted-note { color: var(--muted-foreground); font-style: italic; }
.empty { color: var(--muted-foreground); }
.accent-blue { border-left: 3px solid #2563eb; }
.accent-cyan { border-left: 3px solid #0891b2; }
.accent-green { border-left: 3px solid #059669; }
.accent-amber { border-left: 3px solid #d97706; }
.accent-orange { border-left: 3px solid #ea580c; }
.accent-red { border-left: 3px solid #dc2626; }
.accent-pink { border-left: 3px solid #db2777; }
.accent-violet { border-left: 3px solid #7c3aed; }
.accent-slate { border-left: 3px solid #475569; }
@media (max-width: 900px) { .layout { grid-template-columns: minmax(0, 1fr); } .toc { position: static; max-height: none; } }
@media print {
  .toc { display: none; }
  .layout { display: block; padding: 0; }
  .note { border: none; border-bottom: 1px solid var(--border); border-radius: 0; padding: 12px 0; }
  body { font-size: 12px; }
}"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn note(title: &str, body: &str) -> ExportNote {
        ExportNote {
            id: crate::ids::new_id(),
            title: title.to_owned(),
            body: body.to_owned(),
            folder_id: None,
            folder_path: "Работа".to_owned(),
            tags: vec!["идеи".into()],
            pinned: false,
            archived: false,
            color: Some("blue".into()),
            encrypted: false,
            created_at: Timestamp::from_millis(1_700_000_000_000),
            updated_at: Timestamp::from_millis(1_700_000_000_000),
            content_hash: String::new(),
        }
    }

    #[test]
    fn renders_self_contained_document() {
        let file = render(&[note("Первая", "Текст")], &ExportOptions::default());
        assert_eq!(file.path, DEFAULT_FILE_NAME);
        assert!(file.content.starts_with("<!DOCTYPE html>"));
        assert!(file.content.contains("<style>"), "styles must be inline");
        assert!(
            !file.content.contains("http://") && !file.content.contains("https://cdn"),
            "no external resources"
        );
        assert!(file.content.contains("Первая"));
        assert!(file.content.contains("#идеи"));
    }

    #[test]
    fn escapes_html_in_notes() {
        let file = render(
            &[note("XSS", "<script>alert(1)</script>")],
            &ExportOptions::default(),
        );
        assert!(!file.content.contains("<script>alert"));
        assert!(file.content.contains("&lt;script&gt;"));
    }

    #[test]
    fn converts_markdown_structures() {
        let markdown = "# Заголовок\n\nАбзац с **жирным**, *курсивом* и `кодом`.\n\n\
             - пункт один\n- [ ] задача\n\n1. первый\n2. второй\n\n\
             > цитата\n\n```rust\nfn main() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n---\n";
        let html = markdown_to_html(markdown);
        assert!(html.contains("<h1>Заголовок</h1>"));
        assert!(html.contains("<strong>жирным</strong>"));
        assert!(html.contains("<em>курсивом</em>"));
        assert!(html.contains("<code>кодом</code>"));
        assert!(html.contains("<ul>"));
        assert!(html.contains("type=\"checkbox\" disabled>"));
        assert!(html.contains("<ol>"));
        assert!(html.contains("<blockquote>"));
        assert!(html.contains("<pre><code>fn main() {}"));
        assert!(html.contains("<table>"));
        assert!(html.contains("<th>a</th>"));
        assert!(html.contains("<hr>"));
        assert!(!html.contains("```"));
    }

    #[test]
    fn code_block_content_is_not_interpreted() {
        let html = markdown_to_html("```\n**не жирный** <b>тег</b>\n```");
        assert!(html.contains("**не жирный**"));
        assert!(html.contains("&lt;b&gt;"));
        assert!(!html.contains("<strong>не жирный</strong>"));
    }

    #[test]
    fn inline_code_protects_markup() {
        let html = inline("используй `**не жирный**` здесь");
        assert!(html.contains("<code>**не жирный**</code>"));
        assert!(!html.contains("<strong>не жирный</strong>"));
    }

    #[test]
    fn links_and_images_render() {
        let html = inline("смотри [документ](https://example.com) и ![скрин](img/a.png)");
        assert!(html.contains("href=\"https://example.com\""), "{html}");
        assert!(
            html.contains("<img src=\"img/a.png\" alt=\"скрин\">"),
            "{html}"
        );
    }

    #[test]
    fn nested_emphasis_renders() {
        let html = inline("**жирный со [ссылкой](https://e.com)**");
        assert!(
            html.contains("<strong>жирный со <a href=\"https://e.com\">ссылкой</a></strong>"),
            "{html}"
        );
    }

    #[test]
    fn dangerous_urls_are_neutralized() {
        let html = inline("[клик](javascript:alert(1))");
        assert!(!html.contains("javascript:"), "{html}");
        assert!(html.contains("href=\"#\""));
        // Изображения data: разрешены — это обычный способ вставить картинку.
        let image = inline("![x](data:image/png;base64,AAA)");
        assert!(image.contains("data:image/png;base64,AAA"));
    }

    #[test]
    fn unmatched_markers_stay_text() {
        assert_eq!(inline("звёздочка * не разметка"), "звёздочка * не разметка");
        assert_eq!(inline("[скобка без ссылки"), "[скобка без ссылки");
        assert_eq!(inline("`незакрытый код"), "`незакрытый код");
    }

    #[test]
    fn empty_export_is_still_valid_document() {
        let file = render(&[], &ExportOptions::default());
        assert!(file.content.contains("В выгрузке нет заметок"));
        assert!(file.content.contains("</html>"));
    }

    #[test]
    fn headings_beyond_level_six_are_clamped() {
        let html = markdown_to_html("####### слишком глубоко");
        assert!(!html.contains("<h7>"));
        assert!(html.contains("слишком глубоко"));
    }

    #[test]
    fn rule_detection_does_not_eat_list_items() {
        assert!(is_rule("---"));
        assert!(is_rule("***"));
        assert!(!is_rule("- пункт"));
        assert!(!is_rule("обычный текст"));
    }
}
