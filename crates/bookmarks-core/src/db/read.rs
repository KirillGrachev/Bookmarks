//! Выборки: фильтры, сортировка, пагинация, дерево папок.
//!
//! Запросы собираются динамически, но только параметризованно: каждое условие
//! добавляет в SQL позиционный плейсхолдер `?N`, а значение — в список
//! аргументов. Пользовательский ввод никогда не конкатенируется в SQL.
//! Единственная «текстовая» часть запроса — MATCH-выражение FTS5, которое
//! собирается в [`crate::search`] из экранированных токенов.

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::db::Store;
use crate::domain::note::{Note, NoteSummary};
use crate::domain::{Folder, SortOrder, Tag};
use crate::error::{Error, Result};
use crate::ids::EntityId;
use crate::search::SearchQuery;
use crate::time::Timestamp;

/// Область видимости выборки по признакам «архив» и «корзина».
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Обычные заметки (архив и корзина исключены).
    Active,
    /// Архив.
    Archived,
    /// Корзина.
    Trashed,
    /// Всё, кроме корзины.
    ActiveAndArchived,
    /// Абсолютно всё (экспорт и обслуживание).
    Everything,
}

impl Scope {
    /// SQL-условие по колонкам `archived` / `deleted_at`.
    fn condition(self) -> &'static str {
        match self {
            Self::Active => "archived = 0 AND deleted_at IS NULL",
            Self::Archived => "archived = 1 AND deleted_at IS NULL",
            Self::Trashed => "deleted_at IS NOT NULL",
            Self::ActiveAndArchived => "deleted_at IS NULL",
            Self::Everything => "1 = 1",
        }
    }
}

/// Фильтр списка заметок. Поля комбинируются по «И».
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteFilter {
    /// Ограничить папкой.
    pub folder_id: Option<EntityId>,
    /// Включить вложенные папки.
    pub recursive: bool,
    /// Заметки должны иметь все перечисленные теги.
    pub tags: Vec<EntityId>,
    /// Только закреплённые / только незакреплённые.
    pub pinned: Option<bool>,
    /// Только с цветовой меткой.
    pub color: Option<String>,
    /// Только зашифрованные / только открытые.
    pub encrypted: Option<bool>,
    /// Область видимости.
    pub scope: Option<Scope>,
    /// Изменена не раньше.
    pub updated_since: Option<Timestamp>,
    /// Изменена не позже.
    pub updated_until: Option<Timestamp>,
}

/// Параметры списка заметок.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteQuery {
    /// Фильтр.
    pub filter: NoteFilter,
    /// Полнотекстовый запрос (пустой — без поиска).
    pub query: String,
    /// Сортировка.
    pub sort: SortOrder,
    /// Сколько строк вернуть.
    pub limit: u32,
    /// Сколько строк пропустить.
    pub offset: u32,
}

impl Default for NoteQuery {
    fn default() -> Self {
        Self {
            filter: NoteFilter::default(),
            query: String::new(),
            sort: SortOrder::UpdatedDesc,
            limit: 100,
            offset: 0,
        }
    }
}

/// Страница списка заметок.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePage {
    /// Строки страницы.
    pub items: Vec<NoteSummary>,
    /// Сколько всего строк подходит под фильтр (без учёта `limit`/`offset`).
    pub total: u32,
    /// Текущее смещение.
    pub offset: u32,
    /// Есть ли ещё строки после этой страницы.
    pub has_more: bool,
}

/// Узел дерева папок.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderNode {
    /// Папка.
    pub folder: Folder,
    /// Полный путь от корня через « / » — для хлебных крошек.
    pub path: String,
    /// Глубина: 0 — корень.
    pub depth: u32,
    /// Потомки в порядке `sort_order`.
    pub children: Vec<FolderNode>,
}

/// Агрегаты библиотеки для статусной строки и экрана «О программе».
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    /// Всего активных заметок.
    pub notes: u32,
    /// Суммарное число слов.
    pub words: u64,
    /// Самая старая заметка.
    pub oldest: Option<Timestamp>,
    /// Самое свежее изменение.
    pub newest: Option<Timestamp>,
    /// Папок.
    pub folders: u32,
    /// Тегов.
    pub tags: u32,
    /// Заметок в корзине.
    pub trashed: u32,
    /// Заметок в архиве.
    pub archived: u32,
    /// Зашифрованных заметок.
    pub encrypted: u32,
}

/// Заметка, найденная поиском, с фрагментом совпадения.
///
/// `Eq` намеренно нет: релевантность bm25 — вещественное число.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    /// Краткое описание заметки.
    pub note: NoteSummary,
    /// Фрагмент с маркерами `\u{1}`/`\u{2}` вокруг совпадений (фронтенд рисует `<mark>`).
    pub snippet: String,
    /// Релевантность bm25 (чем меньше, тем лучше).
    pub rank: f64,
}

/// Группа дубликатов.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    /// Общий хеш содержимого.
    pub content_hash: String,
    /// Сколько заметок в группе.
    pub count: u32,
    /// Идентификаторы заметок.
    pub note_ids: Vec<EntityId>,
    /// Заголовок первой заметки — для подписи в списке.
    pub sample_title: String,
}

/// Колонки заметки в едином порядке для всех запросов.
pub(crate) const NOTE_COLUMNS: &str = "n.uid, n.title, n.body, f.uid, n.pinned, n.archived, \
     n.color, n.created_at, n.updated_at, n.deleted_at, n.content_hash, n.word_count, n.encrypted";

/// Колонки краткого описания (без тела).
pub(crate) const SUMMARY_COLUMNS: &str = "n.uid, n.title, f.uid, n.pinned, n.archived, \
     n.color, n.encrypted, n.created_at, n.updated_at";

/// Длина превью в списке, в символах.
const EXCERPT_CHARS: usize = 180;

/// Конструктор WHERE: фрагмент SQL + упорядоченные значения параметров.
///
/// Инвариант: номер `?N` в `sql` всегда равен позиции значения в `values`
/// (нумерация с единицы). Нарушение порядка привело бы к подстановке значений
/// не в те условия, поэтому добавление параметра выполняется одним методом.
#[derive(Default)]
struct Conditions {
    sql: String,
    values: Vec<Box<dyn rusqlite::types::ToSql>>,
}

impl Conditions {
    /// Добавляет условие с одним текстовым параметром.
    fn text(&mut self, sql: &str, value: impl Into<String>) {
        self.values.push(Box::new(value.into()));
        let index = self.values.len();
        self.sql.push_str(&Self::number_placeholders(sql, index));
    }

    /// Добавляет условие с одним целочисленным параметром.
    fn integer(&mut self, sql: &str, value: i64) {
        self.values.push(Box::new(value));
        let index = self.values.len();
        self.sql.push_str(&Self::number_placeholders(sql, index));
    }

    /// Добавляет фрагмент без параметров.
    fn raw(&mut self, sql: &str) {
        self.sql.push_str(sql);
    }

    /// Заменяет единственный `?` в фрагменте на позиционный `?N`.
    ///
    /// Подстановка выполняется по индексу символа, а не поиском по всей строке:
    /// `String::replace` заменила бы и вопросительные знаки внутри уже
    /// добавленных значений, из-за чего в SQL попадало имя параметра вместо
    /// плейсхолдера.
    fn number_placeholders(sql: &str, index: usize) -> String {
        match sql.find('?') {
            Some(position) => format!(
                "{}?{index}{}",
                &sql[..position],
                &sql[position + '?'.len_utf8()..]
            ),
            None => sql.to_owned(),
        }
    }
}

impl Store {
    /// Список заметок по фильтру с пагинацией.
    ///
    /// Тело заметки в список не отдаётся: выбирается только первых 600 символов
    /// для превью, что при тысячах строк уменьшает объём IPC на порядки.
    pub fn list_notes(&self, query: &NoteQuery) -> Result<NotePage> {
        let pooled = self.connection()?;
        let connection = pooled.get();
        let search = SearchQuery::parse(&query.query)?;
        let folder_ids = self.resolve_folder_scope(connection, &query.filter)?;
        let scope = query.filter.scope.unwrap_or(Scope::Active);

        let mut conditions = Conditions::default();
        conditions.raw(&format!(" {} ", scope.condition()));
        build_conditions(
            &mut conditions,
            &query.filter,
            &search,
            folder_ids.as_deref(),
        );

        let from_where = format!(
            "FROM notes n LEFT JOIN folders f ON f.id = n.folder_id WHERE{}",
            conditions.sql
        );
        let total = run_scalar_i64(
            connection,
            &format!("SELECT COUNT(*) {from_where}"),
            &conditions,
        )?;
        let total = u32::try_from(total).unwrap_or(u32::MAX);

        let page_sql = format!(
            "SELECT {SUMMARY_COLUMNS}, substr(n.body, 1, 600) {from_where} \
             ORDER BY {order} LIMIT {limit} OFFSET {offset}",
            order = full_order_by(query.sort),
            limit = query.limit,
            offset = query.offset
        );
        let mut items = run_query(connection, &page_sql, &conditions, map_summary_with_excerpt)?;
        attach_tags(connection, &mut items)?;

        let returned = u32::try_from(items.len()).unwrap_or(u32::MAX);
        Ok(NotePage {
            has_more: query.offset.saturating_add(returned) < total,
            items,
            total,
            offset: query.offset,
        })
    }

    /// Полнотекстовый поиск с ранжированием и фрагментами.
    ///
    /// При пустом текстовом запросе возвращает обычные заметки области
    /// видимости — так у интерфейса одна точка входа для «поиск + фильтры».
    pub fn search_notes(
        &self,
        query: &str,
        filter: &NoteFilter,
        limit: u32,
    ) -> Result<Vec<SearchHit>> {
        let pooled = self.connection()?;
        let connection = pooled.get();
        let search = SearchQuery::parse(query)?;

        // MATCH-выражение может оказаться пустым и при непустом вводе: запрос
        // из одних исключений (`-черновик`) не имеет левого операнда. Тогда
        // MATCH не используется вовсе — иначе FTS5 вернул бы синтаксическую
        // ошибку вместо пустой выдачи.
        let match_expression = search.match_expression();
        if !search.has_text() || match_expression.is_empty() {
            let page = self.list_notes(&NoteQuery {
                filter: filter.clone(),
                limit,
                offset: 0,
                query: String::new(),
                sort: SortOrder::UpdatedDesc,
            })?;
            return Ok(page
                .items
                .into_iter()
                .map(|note| SearchHit {
                    snippet: note.excerpt.clone(),
                    note,
                    rank: 0.0,
                })
                .collect());
        }

        // Операторы `tag:` и `folder:` из запроса превращаются в обычные
        // фильтры: имена разрешаются в идентификаторы одним запросом, поэтому
        // лишние строки отсекает SQL, а не постобработка (иначе LIMIT выдавал
        // бы неполную страницу).
        let mut resolved = filter.clone();
        if resolved.folder_id.is_none() {
            if let Some(id) = first_folder_id_by_names(connection, &search.folder_names())? {
                resolved.folder_id = Some(id);
                resolved.recursive = true;
            }
        }
        let tag_names = search.tag_names();
        if !tag_names.is_empty() {
            let mut tag_ids = tag_ids_by_names(connection, &tag_names)?;
            if tag_ids.len() < tag_names.len() {
                // Тега нет в библиотеке — совпадений заведомо ноль.
                return Ok(Vec::new());
            }
            for tag in tag_ids.drain(..) {
                if !resolved.tags.contains(&tag) {
                    resolved.tags.push(tag);
                }
            }
        }
        let folder_ids = self.resolve_folder_scope(connection, &resolved)?;
        let scope = resolved.scope.unwrap_or(Scope::Active);

        let mut conditions = Conditions::default();
        conditions.raw(&format!(" {} ", scope.condition()));
        conditions.text("AND notes_fts MATCH ?", match_expression);
        // Текстовая часть запроса уже учтена в MATCH выше, поэтому в фильтры
        // передаём пустой поисковый запрос — иначе условие задвоилось бы.
        build_conditions(
            &mut conditions,
            &resolved,
            &SearchQuery::default(),
            folder_ids.as_deref(),
        );
        let date_window = search_date_window(&search);
        let content_filters = search.has_content_filters();
        let flag_filters = search.flag_filters();

        let sql = format!(
            "SELECT {SUMMARY_COLUMNS}, substr(n.body, 1, 600), \
                    snippet(notes_fts, 1, '\u{1}', '\u{2}', ' … ', 24), \
                    bm25(notes_fts, 6.0, 1.0) \
             FROM notes n \
             LEFT JOIN folders f ON f.id = n.folder_id \
             JOIN notes_fts ON notes_fts.rowid = n.id \
             WHERE{where_clause} \
             ORDER BY bm25(notes_fts, 6.0, 1.0) ASC, n.updated_at DESC LIMIT {limit}",
            where_clause = conditions.sql,
            limit = limit
        );

        let mut hits = run_query(connection, &sql, &conditions, |row| {
            let note = map_summary_with_excerpt(row)?;
            let snippet: String = row.get(10)?;
            Ok(SearchHit {
                note,
                rank: row.get(11)?,
                snippet: clean_snippet(&snippet),
            })
        })?;
        attach_tags_to_hits(connection, &mut hits)?;
        // Флаги `is:pinned`, `is:archived`, `is:encrypted`, `is:empty` и
        // `is:untagged` проверяются по уже загруженным строкам: в SQL их
        // пришлось бы дублировать, а данных для этого достаточно на месте.
        if !flag_filters.is_empty() {
            hits.retain(|hit| {
                flag_filters.iter().all(|(flag, negated)| {
                    let present = match flag.as_str() {
                        "pinned" => hit.note.pinned,
                        "archived" => hit.note.archived,
                        "encrypted" => hit.note.encrypted,
                        "empty" => {
                            hit.note.title.trim().is_empty() && hit.note.excerpt.trim().is_empty()
                        }
                        "untagged" => hit.note.tags.is_empty(),
                        "attachment" => false,
                        _ => false,
                    };
                    present != *negated
                })
            });
        }
        if let Some((from, until)) = date_window {
            hits.retain(|hit| {
                let updated = hit.note.updated_at.millis();
                updated >= from && updated <= until
            });
        }
        // Операторы `has:` требуют полного текста, поэтому применяются
        // последними и только к строкам, прошедшим остальные фильтры.
        if content_filters {
            let mut kept = Vec::with_capacity(hits.len());
            for hit in hits {
                let body: String = connection
                    .query_row(
                        "SELECT body FROM notes WHERE uid = ?1",
                        params![hit.note.id],
                        |row| row.get(0),
                    )
                    .unwrap_or_default();
                if crate::search::matches_body(&body, &search.filters) {
                    kept.push(hit);
                }
            }
            hits = kept;
        }
        Ok(hits)
    }

    /// Одна заметка целиком (с тегами).
    pub fn get_note(&self, id: &str) -> Result<Note> {
        let pooled = self.connection()?;
        let connection = pooled.get();
        let sql = format!(
            "SELECT {NOTE_COLUMNS} FROM notes n LEFT JOIN folders f ON f.id = n.folder_id WHERE n.uid = ?1"
        );
        let mut note = connection
            .query_row(&sql, params![id], map_note)
            .optional()?
            .ok_or_else(|| Error::not_found("note", id))?;
        note.tags = note_tag_names(connection, id)?;
        Ok(note)
    }

    /// Несколько заметок одним запросом (глубокие ссылки, проверка синхронизации).
    pub fn get_notes_by_ids(&self, ids: &[EntityId]) -> Result<Vec<Note>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let pooled = self.connection()?;
        let connection = pooled.get();
        let placeholders: Vec<String> = (0..ids.len())
            .map(|index| format!("?{}", index + 1))
            .collect();
        let sql = format!(
            "SELECT {NOTE_COLUMNS} FROM notes n LEFT JOIN folders f ON f.id = n.folder_id \
             WHERE n.uid IN ({}) ORDER BY n.updated_at DESC",
            placeholders.join(", ")
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(ids.iter()), map_note)?;
        let mut notes = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        for note in &mut notes {
            note.tags = note_tag_names(connection, &note.id)?;
        }
        Ok(notes)
    }

    /// Названия тегов заметки.
    pub fn note_tag_names(&self, note_id: &str) -> Result<Vec<String>> {
        let pooled = self.connection()?;
        note_tag_names(pooled.get(), note_id)
    }

    /// Все теги с числом заметок (корзина не учитывается).
    pub fn list_tags(&self) -> Result<Vec<Tag>> {
        let pooled = self.connection()?;
        let mut statement = pooled.get().prepare(
            "SELECT t.uid, t.name, t.color, t.created_at, \
                    (SELECT COUNT(*) FROM note_tags nt JOIN notes n ON n.id = nt.note_id \
                     WHERE nt.tag_id = t.id AND n.deleted_at IS NULL) AS note_count \
             FROM tags t ORDER BY note_count DESC, lower(t.name) ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                created_at: Timestamp::from_millis(row.get(3)?),
                note_count: row.get::<_, i64>(4)?.max(0) as u32,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Один тег по идентификатору.
    pub fn get_tag(&self, id: &str) -> Result<Tag> {
        self.list_tags()?
            .into_iter()
            .find(|tag| tag.id == id)
            .ok_or_else(|| Error::not_found("tag", id))
    }

    /// Все папки плоским списком с числом заметок.
    pub fn list_folders(&self) -> Result<Vec<Folder>> {
        let pooled = self.connection()?;
        list_folders(pooled.get())
    }

    /// Одна папка.
    pub fn get_folder(&self, id: &str) -> Result<Folder> {
        list_folders(self.connection()?.get())?
            .into_iter()
            .find(|folder| folder.id == id)
            .ok_or_else(|| Error::not_found("folder", id))
    }

    /// Дерево папок с путями.
    pub fn folder_tree(&self) -> Result<Vec<FolderNode>> {
        Ok(build_folder_tree(self.list_folders()?))
    }

    /// Идентификатор папки и всех её потомков.
    pub fn folder_descendants(&self, id: &str) -> Result<Vec<EntityId>> {
        let pooled = self.connection()?;
        folder_descendants(pooled.get(), id)
    }

    /// Агрегаты библиотеки.
    pub fn library_stats(&self) -> Result<LibraryStats> {
        let pooled = self.connection()?;
        let connection = pooled.get();
        let one = |sql: &str| -> Result<i64> {
            connection
                .query_row(sql, [], |row| row.get(0))
                .map_err(Error::from)
        };
        let optional = |sql: &str| -> Result<Option<i64>> {
            connection
                .query_row(sql, [], |row| row.get::<_, Option<i64>>(0))
                .map_err(Error::from)
        };
        Ok(LibraryStats {
            notes: one("SELECT COUNT(*) FROM notes WHERE deleted_at IS NULL AND archived = 0")?
                .max(0) as u32,
            words: one("SELECT COALESCE(SUM(word_count), 0) FROM notes WHERE deleted_at IS NULL")?
                .max(0) as u64,
            oldest: optional("SELECT MIN(created_at) FROM notes WHERE deleted_at IS NULL")?
                .map(Timestamp::from_millis),
            newest: optional("SELECT MAX(updated_at) FROM notes WHERE deleted_at IS NULL")?
                .map(Timestamp::from_millis),
            folders: one("SELECT COUNT(*) FROM folders")?.max(0) as u32,
            tags: one("SELECT COUNT(*) FROM tags")?.max(0) as u32,
            trashed: one("SELECT COUNT(*) FROM notes WHERE deleted_at IS NOT NULL")?.max(0) as u32,
            archived: one("SELECT COUNT(*) FROM notes WHERE archived = 1 AND deleted_at IS NULL")?
                .max(0) as u32,
            encrypted: one("SELECT COUNT(*) FROM notes WHERE encrypted = 1 AND deleted_at IS NULL")?
                .max(0) as u32,
        })
    }

    /// Заметки с одинаковым содержимым — кандидаты на объединение.
    pub fn find_duplicates(&self) -> Result<Vec<DuplicateGroup>> {
        let pooled = self.connection()?;
        let mut statement = pooled.get().prepare(
            "SELECT content_hash, COUNT(*) AS total, GROUP_CONCAT(uid, char(10)) AS ids, MIN(title) AS sample \
             FROM notes WHERE deleted_at IS NULL GROUP BY content_hash HAVING total > 1 \
             ORDER BY total DESC, content_hash",
        )?;
        let rows = statement.query_map([], |row| {
            let ids: String = row.get(2)?;
            Ok(DuplicateGroup {
                content_hash: row.get(0)?,
                count: row.get::<_, i64>(1)?.max(0) as u32,
                note_ids: ids
                    .split('\n')
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .collect(),
                sample_title: row.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Заметки в корзине, которые пора удалить безвозвратно.
    pub fn trash_expired_ids(&self, days: u32) -> Result<Vec<EntityId>> {
        let pooled = self.connection()?;
        let cutoff = Timestamp::now()
            .checked_add_days(-i64::from(days))
            .ok_or_else(|| Error::invalid("некорректный срок хранения корзины"))?;
        let mut statement = pooled
            .get()
            .prepare("SELECT uid FROM notes WHERE deleted_at IS NOT NULL AND deleted_at < ?1")?;
        let rows = statement.query_map(params![cutoff.millis()], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Разрешает фильтр по папке в список идентификаторов (с учётом вложенности).
    fn resolve_folder_scope(
        &self,
        connection: &Connection,
        filter: &NoteFilter,
    ) -> Result<Option<Vec<EntityId>>> {
        Ok(match (&filter.folder_id, filter.recursive) {
            (Some(id), true) => Some(folder_descendants(connection, id)?),
            (Some(id), false) => Some(vec![id.clone()]),
            (None, _) => None,
        })
    }
}

// ---------------------------------------------------------------------------
// Построение и исполнение запросов
// ---------------------------------------------------------------------------

fn build_conditions(
    conditions: &mut Conditions,
    filter: &NoteFilter,
    search: &SearchQuery,
    folder_ids: Option<&[EntityId]>,
) {
    // Пустое MATCH-выражение (запрос из одних исключений) в SQL не попадает:
    // FTS5 на пустую строку отвечает синтаксической ошибкой.
    let match_expression = search.match_expression();
    if search.has_text() && !match_expression.is_empty() {
        conditions.text(
            "AND n.id IN (SELECT rowid FROM notes_fts WHERE notes_fts MATCH ?)",
            match_expression,
        );
    }

    if let Some(ids) = folder_ids {
        // Плейсхолдеры нумеруются по мере добавления значений: rusqlite требует
        // строгого соответствия `?N` порядку аргументов.
        let mut placeholders = Vec::with_capacity(ids.len());
        for id in ids {
            conditions.values.push(Box::new(id.clone()));
            placeholders.push(format!("?{}", conditions.values.len()));
        }
        conditions.raw(&format!(
            " AND COALESCE(n.folder_id, 0) IN (SELECT id FROM folders WHERE uid IN ({}))",
            placeholders.join(",")
        ));
    }

    if !filter.tags.is_empty() {
        let mut placeholders = Vec::with_capacity(filter.tags.len());
        for tag in &filter.tags {
            conditions.values.push(Box::new(tag.clone()));
            placeholders.push(format!("?{}", conditions.values.len()));
        }
        conditions.raw(&format!(
            " AND (SELECT COUNT(DISTINCT nt.tag_id) FROM note_tags nt JOIN tags t ON t.id = nt.tag_id \
             WHERE nt.note_id = n.id AND t.uid IN ({})) = {}",
            placeholders.join(","),
            filter.tags.len()
        ));
    }

    if let Some(pinned) = filter.pinned {
        conditions.integer(" AND n.pinned = ?", i64::from(pinned));
    }
    if let Some(encrypted) = filter.encrypted {
        conditions.integer(" AND n.encrypted = ?", i64::from(encrypted));
    }
    if let Some(color) = &filter.color {
        conditions.text(" AND n.color = ?", color.clone());
    }
    if let Some(since) = filter.updated_since {
        conditions.integer(" AND n.updated_at >= ?", since.millis());
    }
    if let Some(until) = filter.updated_until {
        conditions.integer(" AND n.updated_at <= ?", until.millis());
    }
}

/// Добавляет строковый параметр с правильным позиционным номером.
#[allow(dead_code)]
fn push_positional(conditions: &mut Conditions, value: String) {
    conditions.values.push(Box::new(value));
    conditions
        .sql
        .push_str(&format!("?{}", conditions.values.len()));
}
/// Полное выражение сортировки: закреплённые сверху, затем выбранный ключ,
/// затем стабильный хвост (без него страницы «прыгали» бы при равных метках).
pub(crate) fn full_order_by(sort: SortOrder) -> String {
    let key = match sort {
        SortOrder::UpdatedDesc => "n.updated_at DESC",
        SortOrder::UpdatedAsc => "n.updated_at ASC",
        SortOrder::CreatedDesc => "n.created_at DESC",
        SortOrder::CreatedAsc => "n.created_at ASC",
        SortOrder::TitleAsc => "n.title ASC",
        SortOrder::TitleDesc => "n.title DESC",
    };
    // Хвост сортировки — побайтовое сравнение заголовка и `uid`: оно
    // детерминировано (в отличие от NOCASE, который в SQLite работает только с
    // ASCII), поэтому страницы списка не «прыгают» между запросами.
    format!("n.pinned DESC, {key}, n.title ASC, n.uid ASC")
}

fn run_query<T>(
    connection: &Connection,
    sql: &str,
    conditions: &Conditions,
    mut mapper: impl FnMut(&Row<'_>) -> Result<T>,
) -> Result<Vec<T>> {
    let mut statement = connection.prepare(sql)?;
    let references: Vec<&dyn rusqlite::types::ToSql> = conditions
        .values
        .iter()
        .map(|value| value.as_ref() as &dyn rusqlite::types::ToSql)
        .collect();
    let mut rows = statement.query(rusqlite::params_from_iter(references))?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push(mapper(row)?);
    }
    Ok(result)
}

fn run_scalar_i64(connection: &Connection, sql: &str, conditions: &Conditions) -> Result<i64> {
    let references: Vec<&dyn rusqlite::types::ToSql> = conditions
        .values
        .iter()
        .map(|value| value.as_ref() as &dyn rusqlite::types::ToSql)
        .collect();
    connection
        .query_row(sql, rusqlite::params_from_iter(references), |row| {
            row.get(0)
        })
        .map_err(Error::from)
}

pub(crate) fn map_note(row: &Row<'_>) -> rusqlite::Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        title: row.get(1)?,
        body: row.get(2)?,
        folder_id: row.get(3)?,
        tags: Vec::new(),
        pinned: row.get::<_, i64>(4)? != 0,
        archived: row.get::<_, i64>(5)? != 0,
        color: row.get(6)?,
        created_at: Timestamp::from_millis(row.get(7)?),
        updated_at: Timestamp::from_millis(row.get(8)?),
        deleted_at: row.get::<_, Option<i64>>(9)?.map(Timestamp::from_millis),
        content_hash: row.get(10)?,
        word_count: row.get::<_, i64>(11)?.max(0) as u32,
        encrypted: row.get::<_, i64>(12)? != 0,
    })
}

fn map_summary(row: &Row<'_>) -> Result<NoteSummary> {
    Ok(NoteSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        folder_id: row.get(2)?,
        pinned: row.get::<_, i64>(3)? != 0,
        archived: row.get::<_, i64>(4)? != 0,
        color: row.get(5)?,
        encrypted: row.get::<_, i64>(6)? != 0,
        created_at: Timestamp::from_millis(row.get(7)?),
        updated_at: Timestamp::from_millis(row.get(8)?),
        excerpt: String::new(),
        tags: Vec::new(),
    })
}

/// Краткое описание + превью из усечённого тела (колонка 9).
fn map_summary_with_excerpt(row: &Row<'_>) -> Result<NoteSummary> {
    let mut summary = map_summary(row)?;
    let raw_body: String = row.get(9)?;
    summary.excerpt =
        crate::domain::truncate_chars(&crate::search::plaintext(&raw_body), EXCERPT_CHARS);
    Ok(summary)
}

fn note_tag_names(connection: &Connection, note_uid: &str) -> Result<Vec<String>> {
    let mut statement = connection.prepare(
        "SELECT t.name FROM note_tags nt JOIN tags t ON t.id = nt.tag_id \
         JOIN notes n ON n.id = nt.note_id WHERE n.uid = ?1 ORDER BY lower(t.name)",
    )?;
    let rows = statement.query_map(params![note_uid], |row| row.get::<_, String>(0))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::from)
}

/// Дописывает названия тегов в краткие описания (один запрос на заметку).
///
/// Отдельный запрос дешевле большого JOIN с GROUP_CONCAT: список показывает
/// десятки строк на экране, а теги нужны каждой.
fn attach_tags(connection: &Connection, summaries: &mut [NoteSummary]) -> Result<()> {
    for summary in summaries.iter_mut() {
        summary.tags = note_tag_names(connection, &summary.id)?;
    }
    Ok(())
}

fn attach_tags_to_hits(connection: &Connection, hits: &mut [SearchHit]) -> Result<()> {
    for hit in hits.iter_mut() {
        hit.note.tags = note_tag_names(connection, &hit.note.id)?;
    }
    Ok(())
}

fn list_folders(connection: &Connection) -> Result<Vec<Folder>> {
    let mut statement = connection.prepare(
        "SELECT f.uid, f.name, p.uid, f.color, f.icon, f.sort_order, f.created_at, f.updated_at, \
                (SELECT COUNT(*) FROM notes n WHERE n.folder_id = f.id AND n.deleted_at IS NULL AND n.archived = 0) \
         FROM folders f LEFT JOIN folders p ON p.id = f.parent_id \
         ORDER BY f.sort_order ASC, lower(f.name) ASC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(Folder {
            id: row.get(0)?,
            name: row.get(1)?,
            parent_id: row.get(2)?,
            color: row.get(3)?,
            icon: row.get(4)?,
            sort_order: row.get(5)?,
            created_at: Timestamp::from_millis(row.get(6)?),
            updated_at: Timestamp::from_millis(row.get(7)?),
            note_count: row.get::<_, i64>(8)?.max(0) as u32,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::from)
}

fn folder_descendants(connection: &Connection, id: &str) -> Result<Vec<EntityId>> {
    // Обход идёт по внутренним ключам: `parent_id` в базе — INTEGER, а наружу
    // отдаётся `uid`, поэтому сопоставлять их нужно в одном пространстве.
    let mut statement = connection.prepare("SELECT id, uid, parent_id FROM folders")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<i64>>(2)?,
        ))
    })?;
    let entries = rows.collect::<std::result::Result<Vec<_>, _>>()?;

    let root_key = match entries.iter().find(|(_, uid, _)| uid == id) {
        Some((key, _, _)) => *key,
        None => return Ok(vec![id.to_owned()]),
    };

    let mut result = vec![id.to_owned()];
    let mut frontier = vec![root_key];
    while let Some(current) = frontier.pop() {
        for (key, uid, parent) in &entries {
            if *parent == Some(current) && !result.contains(uid) {
                result.push(uid.clone());
                frontier.push(*key);
            }
        }
    }
    Ok(result)
}

/// Приводит фрагмент FTS5 к виду, пригодному для отрисовки.
///
/// `snippet()` оборачивает совпадения символами `\u{1}`/`\u{2}`, которые
/// фронтенд заменяет на `<mark>`. Здесь убираются прочие управляющие символы
/// (переводы строк из исходного Markdown) и схлопываются пробелы — маркеры
/// при этом сохраняются.
fn clean_snippet(raw: &str) -> String {
    let mut normalized = String::with_capacity(raw.len());
    for character in raw.chars() {
        if character == '\u{1}' || character == '\u{2}' {
            normalized.push(character);
        } else if character.is_control() {
            normalized.push(' ');
        } else {
            normalized.push(character);
        }
    }
    let mut result = String::with_capacity(normalized.len());
    let mut previous_was_space = false;
    for character in normalized.chars() {
        if character == '\u{1}' || character == '\u{2}' {
            result.push(character);
            previous_was_space = false;
        } else if character.is_whitespace() {
            if !previous_was_space && !result.is_empty() {
                result.push(' ');
            }
            previous_was_space = true;
        } else {
            result.push(character);
            previous_was_space = false;
        }
    }
    result.trim().to_owned()
}

/// Строит дерево папок из плоского списка.
///
/// Циклы (возможны только после ручного вмешательства в файл базы) не
/// обрезаются молча: ветка, недостижимая из корня, поднимается в корень,
/// чтобы интерфейс не зависал на бесконечной рекурсии.
pub fn build_folder_tree(folders: Vec<Folder>) -> Vec<FolderNode> {
    let index_by_id: HashMap<EntityId, usize> = folders
        .iter()
        .enumerate()
        .map(|(index, folder)| (folder.id.clone(), index))
        .collect();
    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut roots: Vec<usize> = Vec::new();

    for (index, folder) in folders.iter().enumerate() {
        match &folder.parent_id {
            Some(parent) if parent != &folder.id && index_by_id.contains_key(parent) => {
                children.entry(index_by_id[parent]).or_default().push(index);
            }
            _ => roots.push(index),
        }
    }

    let mut reachable = vec![false; folders.len()];
    for root in &roots {
        mark_reachable(*root, &children, &mut reachable);
    }
    for index in 0..folders.len() {
        if !reachable[index] {
            roots.push(index);
            mark_reachable(index, &children, &mut reachable);
        }
    }

    let mut visited = vec![false; folders.len()];
    let mut tree = Vec::new();
    for root in roots {
        if visited[root] {
            continue;
        }
        if let Some(node) = build_node(root, 0, "", &folders, &children, &mut visited) {
            tree.push(node);
        }
    }
    tree
}

fn mark_reachable(index: usize, children: &HashMap<usize, Vec<usize>>, reachable: &mut [bool]) {
    if reachable[index] {
        return;
    }
    reachable[index] = true;
    for child in children.get(&index).cloned().unwrap_or_default() {
        mark_reachable(child, children, reachable);
    }
}

#[allow(clippy::too_many_arguments)]
fn build_node(
    index: usize,
    depth: u32,
    prefix: &str,
    folders: &[Folder],
    children: &HashMap<usize, Vec<usize>>,
    visited: &mut [bool],
) -> Option<FolderNode> {
    if visited[index] {
        return None;
    }
    visited[index] = true;

    let folder = folders[index].clone();
    let path = if prefix.is_empty() {
        folder.name.clone()
    } else {
        format!("{prefix} / {}", folder.name)
    };

    let mut child_indexes = children.get(&index).cloned().unwrap_or_default();
    child_indexes.sort_by(|left, right| {
        folders[*left]
            .sort_order
            .cmp(&folders[*right].sort_order)
            .then_with(|| {
                folders[*left]
                    .name
                    .to_lowercase()
                    .cmp(&folders[*right].name.to_lowercase())
            })
    });

    let mut node = FolderNode {
        path: path.clone(),
        depth,
        folder,
        children: Vec::new(),
    };
    for child in child_indexes {
        if let Some(child_node) = build_node(child, depth + 1, &path, folders, children, visited) {
            node.children.push(child_node);
        }
    }
    Some(node)
}

/// Разрешает названия папок из запроса в идентификатор (первое совпадение).
fn first_folder_id_by_names(connection: &Connection, names: &[String]) -> Result<Option<EntityId>> {
    for name in names {
        let found: Option<EntityId> = connection
            .query_row(
                "SELECT uid FROM folders WHERE lower(name) = lower(?1) ORDER BY sort_order LIMIT 1",
                params![name],
                |row| row.get(0),
            )
            .optional()?;
        if found.is_some() {
            return Ok(found);
        }
    }
    Ok(None)
}

/// Разрешает названия тегов из запроса в идентификаторы.
fn tag_ids_by_names(connection: &Connection, names: &[String]) -> Result<Vec<EntityId>> {
    let mut ids = Vec::with_capacity(names.len());
    for name in names {
        let found: Option<EntityId> = connection
            .query_row(
                "SELECT uid FROM tags WHERE name_lower = lower(?1) LIMIT 1",
                params![name],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(id) = found {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// Диапазон дат из операторов `after:` / `before:`.
fn search_date_window(search: &SearchQuery) -> Option<(i64, i64)> {
    let mut from = i64::MIN;
    let mut until = i64::MAX;
    let mut found = false;
    for filter in &search.filters {
        match filter {
            crate::search::FilterSpec::UpdatedAfter { millis } => {
                from = (*millis).max(from);
                found = true;
            }
            crate::search::FilterSpec::UpdatedBefore { millis } => {
                until = (*millis).min(until);
                found = true;
            }
            _ => {}
        }
    }
    found.then_some((from, until))
}
