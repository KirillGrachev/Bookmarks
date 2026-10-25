//! Парсер поискового запроса.
//!
//! Разбирает строку вида `отчёт tag:работа -черновик "точная фраза"` на
//! текстовые токены (уходят в FTS5) и операторы-фильтры (применяются к
//! результатам). Парсер устойчив к «грязному» вводу: незакрытая кавычка
//! считается обычным словом, неизвестный оператор — тоже словом, пустой
//! запрос не ломает MATCH-выражение.

use chrono::{Duration, NaiveDate, TimeZone, Utc};

use crate::error::{Error, Result};

/// Текстовый токен запроса.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryToken {
    /// Поиск по префиксу: `закуп` → `закуп*`.
    Prefix {
        /// Слово.
        word: String,
        /// Исключать совпадения.
        negated: bool,
    },
    /// Поиск точной фразы (содержимое кавычек).
    Phrase {
        /// Фраза.
        phrase: String,
        /// Исключать совпадения.
        negated: bool,
    },
}

impl QueryToken {
    /// «Сырое» значение токена — для сравнения с названиями тегов и папок.
    pub fn value(&self) -> &str {
        match self {
            Self::Prefix { word, .. } => word,
            Self::Phrase { phrase, .. } => phrase,
        }
    }
}

/// Оператор-фильтр (не участвует в полнотекстовом поиске).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterSpec {
    /// `tag:имя` или `#имя`.
    Tag {
        /// Название тега (без `#`).
        name: String,
        /// Отрицание (`-tag:имя`).
        negated: bool,
    },
    /// `folder:имя`.
    Folder {
        /// Название папки.
        name: String,
        /// Отрицание.
        negated: bool,
    },
    /// `is:флаг`.
    Flag {
        /// Имя флага: `pinned`, `archived`, `encrypted`, `empty`, `attachment`.
        flag: String,
        /// Отрицание.
        negated: bool,
    },
    /// `has:вид` — по содержимому Markdown.
    Has {
        /// Вид содержимого: `image`, `link`, `code`, `task`, `table`, `attachment`.
        kind: String,
        /// Отрицание.
        negated: bool,
    },
    /// `color:имя`.
    Color {
        /// Имя цвета из палитры.
        name: String,
        /// Отрицание.
        negated: bool,
    },
    /// `after:дата` — изменённые не раньше (включительно).
    UpdatedAfter {
        /// Миллисекунды UTC.
        millis: i64,
    },
    /// `before:дата` — изменённые не позже (включительно).
    UpdatedBefore {
        /// Миллисекунды UTC.
        millis: i64,
    },
}

/// Разобранный поисковый запрос.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    /// Текстовые токены в порядке ввода.
    pub text: Vec<QueryToken>,
    /// Операторы-фильтры в порядке ввода.
    pub filters: Vec<FilterSpec>,
}

/// Известные операторы.
const KNOWN_OPERATORS: [&str; 7] = ["tag", "folder", "is", "has", "color", "after", "before"];

impl SearchQuery {
    /// Разбирает строку запроса.
    pub fn parse(raw: &str) -> Result<Self> {
        let mut parsed = Self::default();
        let mut characters = raw.char_indices().peekable();

        while let Some((start, character)) = characters.next() {
            if character.is_whitespace() {
                continue;
            }
            if character == '"' {
                let phrase = read_quoted(raw, start + 1);
                let end = phrase.end.min(raw.len());
                advance_to(&mut characters, end);
                if !phrase.value.trim().is_empty() {
                    parsed.text.push(QueryToken::Phrase {
                        phrase: phrase.value.trim().to_owned(),
                        negated: false,
                    });
                }
                continue;
            }

            let token = read_word(raw, start);
            advance_to(&mut characters, token.end);
            classify_token(token.value, &mut parsed)?;
        }

        Ok(parsed)
    }

    /// Есть ли текстовая часть (иначе MATCH-выражение строить нельзя).
    pub fn has_text(&self) -> bool {
        !self.text.is_empty()
    }

    /// Есть ли операторы-фильтры.
    pub fn has_filters(&self) -> bool {
        !self.filters.is_empty()
    }

    /// Есть ли операторы `has:`, требующие полного текста заметки.
    pub fn has_content_filters(&self) -> bool {
        self.filters
            .iter()
            .any(|filter| matches!(filter, FilterSpec::Has { .. }))
    }

    /// Флаги из `is:` парами (имя, отрицание).
    pub fn flag_filters(&self) -> Vec<(String, bool)> {
        self.filters
            .iter()
            .filter_map(|filter| match filter {
                FilterSpec::Flag { flag, negated } => Some((flag.clone(), *negated)),
                _ => None,
            })
            .collect()
    }

    /// MATCH-выражение для FTS5.
    ///
    /// Возвращает `None` для пустого текстового запроса: вызывающий код обязан
    /// в этом случае не использовать MATCH вовсе (пустая строка — ошибка FTS5).
    /// Все значения экранируются двойными кавычками, поэтому ни один символ
    /// пользовательского ввода не может изменить семантику запроса.
    pub fn match_expression(&self) -> String {
        let mut positives: Vec<String> = Vec::new();
        let mut negations: Vec<String> = Vec::new();
        for token in &self.text {
            let (value, suffix) = match token {
                QueryToken::Prefix { word, .. } => (word.as_str(), "*"),
                QueryToken::Phrase { phrase, .. } => (phrase.as_str(), ""),
            };
            let escaped = escape_fts_value(value);
            if escaped.is_empty() {
                continue;
            }
            let term = format!("\"{escaped}\"{suffix}");
            let negated = matches!(
                token,
                QueryToken::Prefix { negated: true, .. } | QueryToken::Phrase { negated: true, .. }
            );
            if negated {
                negations.push(term);
            } else {
                positives.push(term);
            }
        }

        // Синтаксис FTS5 допускает только бинарный `NOT` (`A NOT B`): унарный
        // минус и `AND NOT` приводят к ошибке разбора, а запрос из одних
        // исключений не имеет левого операнда и возвращает пустую строку —
        // вызывающий код в этом случае не использует MATCH вовсе.
        let mut expression = positives.join(" AND ");
        for negation in negations {
            if expression.is_empty() {
                break;
            }
            expression.push_str(" NOT ");
            expression.push_str(&negation);
        }
        expression
    }

    /// Названия тегов из запроса (без учёта отрицаний).
    pub fn tag_names(&self) -> Vec<String> {
        self.filters
            .iter()
            .filter_map(|filter| match filter {
                FilterSpec::Tag {
                    name,
                    negated: false,
                } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    /// Названия папок из запроса (без учёта отрицаний).
    pub fn folder_names(&self) -> Vec<String> {
        self.filters
            .iter()
            .filter_map(|filter| match filter {
                FilterSpec::Folder {
                    name,
                    negated: false,
                } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }
}

/// Экранирует значение для MATCH-выражения FTS5.
///
/// Внутри строки в двойных кавычках спецсимволом является только сама кавычка —
/// она удваивается. Управляющие символы отбрасываются.
fn escape_fts_value(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_control() {
            continue;
        }
        if character == '"' {
            escaped.push('"');
        }
        escaped.push(character);
    }
    escaped
}

/// Промежуточный результат чтения токена.
struct RawToken<'input> {
    value: &'input str,
    end: usize,
}

/// Читает слово до пробела или кавычки.
fn read_word(input: &str, start: usize) -> RawToken<'_> {
    let rest = &input[start..];
    let length = rest
        .char_indices()
        .find(|(_, character)| character.is_whitespace() || *character == '"')
        .map(|(index, _)| index)
        .unwrap_or(rest.len());
    RawToken {
        value: &rest[..length],
        end: start + length,
    }
}

/// Читает содержимое кавычек; незакрытая кавычка берётся до конца строки.
fn read_quoted(input: &str, start: usize) -> RawToken<'_> {
    let rest = &input[start..];
    match rest.find('"') {
        Some(index) => RawToken {
            value: &rest[..index],
            end: start + index + 1,
        },
        None => RawToken {
            value: rest,
            end: input.len(),
        },
    }
}

fn advance_to(iter: &mut std::iter::Peekable<std::str::CharIndices>, end: usize) {
    while let Some((index, _)) = iter.peek() {
        if *index < end {
            iter.next();
        } else {
            break;
        }
    }
}

/// Распределяет токен между текстом и фильтрами.
fn classify_token(token: &str, parsed: &mut SearchQuery) -> Result<()> {
    if token.is_empty() {
        return Ok(());
    }
    // Ведущий `-` — отрицание (`-черновик`), как в привычных поисковых строках;
    // дефисы по краям токена после него считаются пунктуацией и срезаются,
    // чтобы «слово-» и «-слово-» не превращались в мусорные запросы.
    let (negated, body) = match token.strip_prefix('-') {
        Some(rest) => (true, rest.trim_matches('-')),
        None => (false, token.trim_end_matches('-')),
    };
    if body.is_empty() {
        return Ok(());
    }

    // `#тег` — короткая форма фильтра по тегу.
    if let Some(name) = body.strip_prefix('#') {
        if !name.is_empty() && !name.chars().any(char::is_whitespace) {
            parsed.filters.push(FilterSpec::Tag {
                name: crate::domain::normalize_label(name).to_lowercase(),
                negated,
            });
            return Ok(());
        }
    }

    if let Some((operator, value)) = split_operator(body) {
        if KNOWN_OPERATORS.contains(&operator.as_str()) && !value.is_empty() {
            return push_operator_filter(parsed, &operator, value, negated);
        }
    }

    // Обычное слово (или неизвестный оператор — ищем как есть).
    let cleaned = strip_fts_syntax(body);
    if cleaned.is_empty() {
        return Ok(());
    }
    parsed.text.push(QueryToken::Prefix {
        word: cleaned,
        negated,
    });
    Ok(())
}

fn push_operator_filter(
    parsed: &mut SearchQuery,
    operator: &str,
    value: &str,
    negated: bool,
) -> Result<()> {
    let normalized = crate::domain::normalize_label(value);
    match operator {
        "tag" => parsed.filters.push(FilterSpec::Tag {
            name: normalized.trim_start_matches('#').to_lowercase(),
            negated,
        }),
        "folder" => parsed.filters.push(FilterSpec::Folder {
            name: normalized.to_lowercase(),
            negated,
        }),
        "color" => parsed.filters.push(FilterSpec::Color {
            name: normalized.to_lowercase(),
            negated,
        }),
        "is" => {
            let flag = normalized.to_lowercase();
            if !is_known_flag(&flag) {
                return Err(Error::invalid(format!("неизвестный флаг is:{flag}")));
            }
            parsed.filters.push(FilterSpec::Flag { flag, negated });
        }
        "has" => {
            let kind = normalized.to_lowercase();
            if !is_known_content_kind(&kind) {
                return Err(Error::invalid(format!(
                    "неизвестный вид содержимого has:{kind}"
                )));
            }
            parsed.filters.push(FilterSpec::Has { kind, negated });
        }
        "after" | "before" => {
            let end_of_day = operator == "before";
            let millis = parse_date_boundary(&normalized, end_of_day).ok_or_else(|| {
                Error::invalid(format!(
                    "не удалось разобрать дату в {operator}:{normalized}"
                ))
            })?;
            let filter = if end_of_day {
                FilterSpec::UpdatedBefore { millis }
            } else {
                FilterSpec::UpdatedAfter { millis }
            };
            parsed.filters.push(filter);
        }
        _ => unreachable!("checked against KNOWN_OPERATORS"),
    }
    Ok(())
}

/// Признак, допустимый в `is:`.
pub fn is_known_flag(flag: &str) -> bool {
    matches!(
        flag,
        "pinned" | "archived" | "encrypted" | "empty" | "attachment" | "untagged"
    )
}

/// Вид содержимого, допустимый в `has:`.
pub fn is_known_content_kind(kind: &str) -> bool {
    matches!(
        kind,
        "image" | "link" | "code" | "task" | "table" | "attachment"
    )
}

/// Отделяет оператор от значения (`tag:работа` → `("tag", "работа")`).
fn split_operator(token: &str) -> Option<(String, &str)> {
    let index = token.find(':')?;
    let (operator, value) = token.split_at(index);
    if operator.is_empty()
        || !operator
            .chars()
            .all(|character| character.is_ascii_alphabetic())
    {
        return None;
    }
    Some((operator.to_ascii_lowercase(), &value[1..]))
}

/// Убирает символы синтаксиса FTS5, чтобы пользовательский ввод не ломал MATCH.
fn strip_fts_syntax(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            !(matches!(character, '*' | '^' | '(' | ')' | '{' | '}' | ':' | '"')
                || character.is_control())
        })
        .collect()
}

/// Разбирает значение даты: ISO (`2026-01-31`), `дд.мм.гггг`, компактное
/// (`20260131`) или относительное (`today`, `yesterday`, `7d`, `2w`,
/// `сегодня`, `вчера`, `неделя`, `месяц`, `год`).
///
/// `end_of_day` задаёт границу суток: для `after:` нужно начало дня, для
/// `before:` — конец, иначе «изменённые до 31 января» потеряли бы весь день.
pub fn parse_date_value(value: &str) -> Option<i64> {
    parse_date_boundary(value, false)
}

/// Разбирает дату с явным указанием границы суток.
pub fn parse_date_boundary(value: &str, end_of_day: bool) -> Option<i64> {
    let lowered = value.trim().to_lowercase();
    let today = Utc::now().date_naive();
    let date = match lowered.as_str() {
        "today" | "сегодня" | "0d" => Some(today),
        "yesterday" | "вчера" => Some(today - Duration::days(1)),
        "week" | "неделя" | "7d" | "1w" => Some(today - Duration::days(7)),
        "month" | "месяц" | "30d" | "1mo" => Some(today - Duration::days(30)),
        "year" | "год" | "365d" | "1y" => Some(today - Duration::days(365)),
        _ => parse_relative_offset(&lowered)
            .map(|days| today - Duration::days(days))
            .or_else(|| NaiveDate::parse_from_str(&lowered, "%Y-%m-%d").ok())
            .or_else(|| NaiveDate::parse_from_str(&lowered, "%d.%m.%Y").ok())
            .or_else(|| NaiveDate::parse_from_str(&lowered, "%Y%m%d").ok()),
    }?;

    let (hour, minute, second) = if end_of_day { (23, 59, 59) } else { (0, 0, 0) };
    let naive = date.and_hms_opt(hour, minute, second)?;
    Some(Utc.from_utc_datetime(&naive).timestamp_millis())
}

/// `7d`, `2w`, `3mo`, `1y` → число суток.
fn parse_relative_offset(value: &str) -> Option<i64> {
    let (number, unit) = value.split_at(value.find(|character: char| !character.is_ascii_digit())?);
    let count: i64 = number.parse().ok()?;
    let days = match unit {
        "d" => count,
        "w" => count * 7,
        "mo" | "m" => count * 30,
        "y" => count * 365,
        _ => return None,
    };
    Some(days)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_words_as_prefix_terms() {
        let query = SearchQuery::parse("отчёт закупки").expect("parses");
        assert_eq!(query.text.len(), 2);
        assert!(!query.has_filters());
        assert_eq!(query.match_expression(), "\"отчёт\"* AND \"закупки\"*");
    }

    #[test]
    fn parses_quoted_phrase() {
        let query = SearchQuery::parse("\"итоги года\" 2026").expect("parses");
        assert_eq!(
            query.text,
            vec![
                QueryToken::Phrase {
                    phrase: "итоги года".into(),
                    negated: false
                },
                QueryToken::Prefix {
                    word: "2026".into(),
                    negated: false
                }
            ]
        );
        assert_eq!(query.match_expression(), "\"итоги года\" AND \"2026\"*");
    }

    #[test]
    fn unterminated_quote_degrades_to_phrase() {
        let query = SearchQuery::parse("\"незакрытая фраза").expect("parses");
        assert_eq!(query.match_expression(), "\"незакрытая фраза\"");
    }

    #[test]
    fn leading_hyphen_is_negation_and_edge_hyphens_are_trimmed() {
        // Поведение задокументировано: ведущий минус всегда означает
        // «исключить», поэтому слово, начинающееся с дефиса, ищется без него.
        let query = SearchQuery::parse("текст -и-ушёл").expect("parses");
        assert_eq!(query.match_expression(), "\"текст\"* NOT \"и-ушёл\"*");
        // Запрос только из исключений не даёт валидного MATCH — выражение
        // пустое, и поиск в этом случае выполняется без полнотекстовой части.
        let only_negation = SearchQuery::parse("-и-ушёл").expect("parses");
        assert_eq!(only_negation.match_expression(), "");
        let trailing = SearchQuery::parse("слово-").expect("parses");
        assert_eq!(trailing.match_expression(), "\"слово\"*");
        let negated = SearchQuery::parse("заметки -черновик").expect("parses");
        assert_eq!(negated.match_expression(), "\"заметки\"* NOT \"черновик\"*");
    }

    #[test]
    fn negation_becomes_not_term() {
        let query = SearchQuery::parse("отчёт -черновик").expect("parses");
        assert_eq!(query.match_expression(), "\"отчёт\"* NOT \"черновик\"*");
    }

    #[test]
    fn operators_are_extracted_from_text() {
        let query = SearchQuery::parse(
            "встреча tag:работа folder:Проекты is:pinned has:task color:red -tag:архив",
        )
        .expect("parses");
        assert_eq!(query.text.len(), 1, "operators must not reach FTS");
        assert_eq!(query.tag_names(), vec!["работа".to_string()]);
        assert_eq!(query.folder_names(), vec!["проекты".to_string()]);
        assert!(query.filters.contains(&FilterSpec::Flag {
            flag: "pinned".into(),
            negated: false
        }));
        assert!(query.filters.contains(&FilterSpec::Has {
            kind: "task".into(),
            negated: false
        }));
        assert!(query.filters.contains(&FilterSpec::Color {
            name: "red".into(),
            negated: false
        }));
        assert!(query.filters.contains(&FilterSpec::Tag {
            name: "архив".into(),
            negated: true
        }));
        assert_eq!(query.match_expression(), "\"встреча\"*");
    }

    #[test]
    fn hash_shortcut_maps_to_tag_filter() {
        let query = SearchQuery::parse("#Работа").expect("parses");
        assert!(query.text.is_empty());
        assert_eq!(query.tag_names(), vec!["работа".to_string()]);
    }

    #[test]
    fn unknown_operator_stays_a_word() {
        let query = SearchQuery::parse("author:pushkin").expect("parses");
        assert_eq!(
            query.text,
            vec![QueryToken::Prefix {
                word: "authorpushkin".into(),
                negated: false
            }]
        );
        assert!(!query.has_filters());
    }

    #[test]
    fn unknown_flag_is_reported() {
        let error = SearchQuery::parse("is:beautiful").expect_err("must fail");
        assert!(matches!(error, Error::InvalidInput { .. }));
    }

    #[test]
    fn fts_syntax_characters_are_stripped() {
        let query = SearchQuery::parse("a*b (c) {d} ^e").expect("parses");
        assert_eq!(
            query.match_expression(),
            "\"ab\"* AND \"c\"* AND \"d\"* AND \"e\"*"
        );
    }

    #[test]
    fn quotes_split_tokens_instead_of_breaking_match() {
        // Кавычка в середине слова — граница токена: «код» ищется по префиксу,
        // «кавычка» как фраза. Главное — MATCH остаётся синтаксически валидным.
        let query = SearchQuery::parse("код\"кавычка").expect("parses");
        assert_eq!(query.match_expression(), "\"код\"* AND \"кавычка\"");
    }

    #[test]
    fn match_values_escape_quotes() {
        // Экранирование удваивает кавычки по правилам FTS5 — иначе MATCH
        // ломался бы на первом же тексте с кавычками.
        assert_eq!(
            escape_fts_value("цитата \"внутри\""),
            "цитата \"\"внутри\"\""
        );
        assert_eq!(escape_fts_value("обычный текст"), "обычный текст");
        // Управляющие символы в запрос не проходят вовсе.
        assert_eq!(escape_fts_value("a\u{0}b\u{7}c"), "abc");
    }

    #[test]
    fn quotes_act_as_token_boundaries() {
        // Кавычка — граница токена: ввод «сказал-"привет"-и-ушёл» даёт три
        // токена (слово, фраза, исключение). Это намеренно: кавычки в запросе
        // означают поиск фразы, а не литеру внутри слова.
        let query = SearchQuery::parse("сказал-\"привет\"-и-ушёл").expect("parses");
        assert_eq!(query.text.len(), 3);
        assert_eq!(
            query.match_expression(),
            "\"сказал\"* AND \"привет\" NOT \"и-ушёл\"*"
        );
    }

    #[test]
    fn phrase_broken_by_quote_stays_valid() {
        // Незакрытая/вложенная кавычка делит ввод на два токена: результат
        // хуже исходного намерения, но MATCH остаётся синтаксически верным и
        // запрос не падает.
        let query = SearchQuery::parse("\"цитата \"\"внутри").expect("parses");
        assert_eq!(query.match_expression(), "\"цитата\" AND \"внутри\"");
    }

    #[test]
    fn empty_query_has_no_match_expression() {
        for raw in ["", "   ", "\n\t"] {
            let query = SearchQuery::parse(raw).expect("parses");
            assert!(!query.has_text());
            assert_eq!(query.match_expression(), "");
        }
    }

    #[test]
    fn parses_dates() {
        assert!(parse_date_boundary("2026-01-31", false).is_some());
        assert!(parse_date_boundary("31.12.2025", false).is_some());
        assert!(parse_date_boundary("20260131", false).is_some());
        assert!(parse_date_boundary("today", false).is_some());
        assert!(parse_date_boundary("7d", false).is_some());
        assert!(parse_date_boundary("2w", false).is_some());
        assert!(parse_date_boundary("вчера", false).is_some());
        assert!(parse_date_boundary("не-дата", false).is_none());
        // Конец суток позже начала тех же суток.
        let start = parse_date_boundary("2026-01-31", false).expect("start");
        let end = parse_date_boundary("2026-01-31", true).expect("end");
        assert!(end > start);
    }

    #[test]
    fn date_filters_land_in_filters_not_text() {
        let query = SearchQuery::parse("after:2026-01-01 before:2026-02-01").expect("parses");
        assert!(query.text.is_empty());
        assert_eq!(query.filters.len(), 2);
        assert!(matches!(query.filters[0], FilterSpec::UpdatedAfter { .. }));
        assert!(matches!(query.filters[1], FilterSpec::UpdatedBefore { .. }));
    }

    #[test]
    fn before_is_not_reported_as_unknown_date() {
        assert!(SearchQuery::parse("before:today").is_ok());
        assert!(SearchQuery::parse("after:tomorrow").is_err());
    }

    #[test]
    fn collects_multiple_flags() {
        let query = SearchQuery::parse("is:pinned is:encrypted").expect("parses");
        assert!(query.filters.contains(&FilterSpec::Flag {
            flag: "pinned".into(),
            negated: false
        }));
        assert!(query.filters.contains(&FilterSpec::Flag {
            flag: "encrypted".into(),
            negated: false
        }));
        assert!(query.text.is_empty());
    }
}
