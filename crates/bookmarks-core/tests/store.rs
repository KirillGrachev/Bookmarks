//! Интеграционные тесты хранилища: реальный SQLite, реальные запросы.
//!
//! Проверяют связки, которые не видны из модульных тестов: триггеры поискового
//! индекса, каскадные удаления, уникальность имён папок, пагинация списка и
//! поведение корзины.

use bookmarks_core::db::{NoteFilter, NoteQuery, Scope, Store};
use bookmarks_core::domain::note::{NoteDraft, NotePatch};
use bookmarks_core::domain::{FolderDraft, SortOrder, TagDraft};

fn store() -> Store {
    Store::open_in_memory().expect("in-memory store")
}

fn note(title: &str, body: &str) -> NoteDraft {
    NoteDraft {
        title: Some(title.to_owned()),
        body: Some(body.to_owned()),
        ..Default::default()
    }
}

#[test]
fn creates_updates_and_reads_note() {
    let store = store();
    let created = store
        .create_note(note("Первая", "молоко хлеб"))
        .expect("create");
    assert_eq!(created.title, "Первая");
    assert_eq!(created.word_count, 2);
    assert!(!created.content_hash.is_empty());
    assert!(created.tags.is_empty());

    let updated = store
        .update_note(
            &created.id,
            NotePatch {
                body: Some("молоко хлеб яйца".to_owned()),
                pinned: Some(true),
                ..Default::default()
            },
        )
        .expect("update");
    assert_eq!(updated.word_count, 3);
    assert!(updated.pinned);
    assert_ne!(updated.content_hash, created.content_hash);
    assert!(updated.updated_at >= created.updated_at);

    let fetched = store.get_note(&created.id).expect("get");
    assert_eq!(fetched.body, "молоко хлеб яйца");
    assert!(store.get_note("несуществующий").is_err());
}

#[test]
fn title_is_derived_from_first_heading() {
    let store = store();
    let created = store
        .create_note(NoteDraft {
            title: None,
            body: Some("# Список покупок\n- молоко".to_owned()),
            ..Default::default()
        })
        .expect("create");
    assert_eq!(created.title, "Список покупок");
}

#[test]
fn normalizes_labels_and_rejects_bad_input() {
    let store = store();
    let created = store
        .create_note(note("  Много   пробелов  ", "текст"))
        .expect("create");
    assert_eq!(created.title, "Много пробелов");

    let error = store
        .create_note(note(&"x".repeat(400), "текст"))
        .expect_err("too long");
    let payload: bookmarks_core::error::ErrorPayload = error.into();
    assert_eq!(payload.kind, "invalid_input");
    assert_eq!(payload.field.as_deref(), Some("title"));
}

#[test]
fn folder_scope_and_recursion_work() {
    let store = store();
    let root = store
        .create_folder(FolderDraft {
            name: "Работа".into(),
            ..Default::default()
        })
        .expect("root");
    let child = store
        .create_folder(FolderDraft {
            name: "Проекты".into(),
            parent_id: Some(root.id.clone()),
            ..Default::default()
        })
        .expect("child");
    store
        .create_note(NoteDraft {
            folder_id: Some(root.id.clone()),
            ..note("В корне", "а")
        })
        .expect("note in root");
    store
        .create_note(NoteDraft {
            folder_id: Some(child.id.clone()),
            ..note("Во вложенной", "б")
        })
        .expect("note in child");
    store.create_note(note("Без папки", "в")).expect("rootless");

    let only_root = store
        .list_notes(&NoteQuery {
            filter: NoteFilter {
                folder_id: Some(root.id.clone()),
                ..Default::default()
            },
            ..Default::default()
        })
        .expect("list");
    assert_eq!(only_root.total, 1);
    assert_eq!(only_root.items[0].title, "В корне");

    let recursive = store
        .list_notes(&NoteQuery {
            filter: NoteFilter {
                folder_id: Some(root.id.clone()),
                recursive: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .expect("list");
    assert_eq!(recursive.total, 2);
}

#[test]
fn folder_names_are_unique_case_insensitively_within_parent() {
    let store = store();
    store
        .create_folder(FolderDraft {
            name: "Работа".into(),
            ..Default::default()
        })
        .expect("first");
    let duplicate = store.create_folder(FolderDraft {
        name: "работа".into(),
        ..Default::default()
    });
    match duplicate {
        Err(error) => {
            let payload: bookmarks_core::error::ErrorPayload = error.into();
            assert_eq!(payload.field.as_deref(), Some("name"));
        }
        Ok(_) => panic!("duplicate folder name must be rejected"),
    }

    let other = store
        .create_folder(FolderDraft {
            name: "Личное".into(),
            ..Default::default()
        })
        .expect("sibling");
    store
        .create_folder(FolderDraft {
            name: "Работа".into(),
            parent_id: Some(other.id.clone()),
            ..Default::default()
        })
        .expect("same name under another parent");
}

#[test]
fn deleting_folder_lifts_notes_and_children() {
    let store = store();
    let parent = store
        .create_folder(FolderDraft {
            name: "Родитель".into(),
            ..Default::default()
        })
        .expect("parent");
    let child = store
        .create_folder(FolderDraft {
            name: "Потомок".into(),
            parent_id: Some(parent.id.clone()),
            ..Default::default()
        })
        .expect("child");
    let created = store
        .create_note(NoteDraft {
            folder_id: Some(child.id.clone()),
            ..note("Заметка", "текст")
        })
        .expect("note");

    let outcome = store.delete_folder(&child.id).expect("delete");
    assert_eq!(outcome.moved_notes, 1);
    let after = store.get_note(&created.id).expect("note survived");
    assert!(after.folder_id.is_none(), "note must move to the root");

    let outcome = store.delete_folder(&parent.id).expect("delete parent");
    assert_eq!(outcome.moved_notes, 0);
    assert_eq!(outcome.moved_folders, 0);
}

#[test]
fn folder_cycle_is_rejected() {
    let store = store();
    let a = store
        .create_folder(FolderDraft {
            name: "A".into(),
            ..Default::default()
        })
        .expect("a");
    let b = store
        .create_folder(FolderDraft {
            name: "B".into(),
            parent_id: Some(a.id.clone()),
            ..Default::default()
        })
        .expect("b");
    let c = store
        .create_folder(FolderDraft {
            name: "C".into(),
            parent_id: Some(b.id.clone()),
            ..Default::default()
        })
        .expect("c");

    assert!(
        store.move_folder(&a.id, Some(&c.id)).is_err(),
        "moving A into its own descendant must fail"
    );
    assert!(
        store.move_folder(&a.id, Some(&a.id)).is_err(),
        "self-parenting must fail"
    );
    // Легальное перемещение работает.
    store.move_folder(&c.id, None).expect("lift to root");
    let tree = store.folder_tree().expect("tree");
    assert_eq!(tree.len(), 2);
}

#[test]
fn folder_tree_reports_paths_and_depths() {
    let store = store();
    let root = store
        .create_folder(FolderDraft {
            name: "Работа".into(),
            ..Default::default()
        })
        .expect("root");
    store
        .create_folder(FolderDraft {
            name: "Проекты".into(),
            parent_id: Some(root.id.clone()),
            ..Default::default()
        })
        .expect("child");
    let tree = store.folder_tree().expect("tree");
    assert_eq!(tree.len(), 1);
    assert_eq!(tree[0].path, "Работа");
    assert_eq!(tree[0].depth, 0);
    assert_eq!(tree[0].children.len(), 1);
    assert_eq!(tree[0].children[0].path, "Работа / Проекты");
    assert_eq!(tree[0].children[0].depth, 1);
}

#[test]
fn tags_are_deduplicated_and_counted() {
    let store = store();
    let created = store
        .create_note(NoteDraft {
            tags: vec!["#Работа".into(), "работа".into(), "идеи".into()],
            ..note("Заметка", "текст")
        })
        .expect("create");
    assert_eq!(
        created.tags.len(),
        2,
        "case-insensitive duplicates must merge"
    );

    let tags = store.list_tags().expect("tags");
    let work = tags
        .iter()
        .find(|tag| tag.name.to_lowercase() == "работа")
        .expect("tag");
    assert_eq!(work.note_count, 1);
    assert_eq!(work.name, "Работа", "first spelling wins");

    store
        .create_tag(TagDraft {
            name: "#новости".into(),
            color: Some("blue".into()),
        })
        .expect("create tag");
    assert!(store
        .create_tag(TagDraft {
            name: "новости".into(),
            ..Default::default()
        })
        .is_err());
}

#[test]
fn renaming_tag_merges_with_existing() {
    let store = store();
    store
        .create_tag(TagDraft {
            name: "идеи".into(),
            ..Default::default()
        })
        .expect("first");
    store
        .create_tag(TagDraft {
            name: "мысли".into(),
            ..Default::default()
        })
        .expect("second");
    let created = store
        .create_note(NoteDraft {
            tags: vec!["мысли".into()],
            ..note("Заметка", "текст")
        })
        .expect("note");

    let thoughts = store.list_tags().expect("tags");
    let thoughts_id = thoughts
        .iter()
        .find(|tag| tag.name == "мысли")
        .expect("tag")
        .id
        .clone();
    store.rename_tag(&thoughts_id, "идеи").expect("rename");

    let tags = store.list_tags().expect("tags");
    assert_eq!(tags.len(), 1, "tags must merge");
    assert_eq!(tags[0].note_count, 1);
    let refreshed = store.get_note(&created.id).expect("note");
    assert_eq!(refreshed.tags.len(), 1);
}

#[test]
fn deleting_tag_detaches_notes() {
    let store = store();
    let created = store
        .create_note(NoteDraft {
            tags: vec!["временный".into()],
            ..note("Заметка", "текст")
        })
        .expect("note");
    let tag = store.list_tags().expect("tags").remove(0);
    let detached = store.delete_tag(&tag.id).expect("delete");
    assert_eq!(detached, 1);
    assert!(store.get_note(&created.id).expect("note").tags.is_empty());
    assert!(store.list_tags().expect("tags").is_empty());
}

#[test]
fn full_text_search_finds_words_and_prefixes() {
    let store = store();
    store
        .create_note(note("Список покупок", "молоко хлеб яйца"))
        .expect("first");
    store
        .create_note(note("Работа", "встреча с командой по закупкам"))
        .expect("second");
    store
        .create_note(note("Личное", "позвонить врачу"))
        .expect("third");

    let hits = store
        .search_notes("закуп", &NoteFilter::default(), 20)
        .expect("search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].note.title, "Работа");
    assert!(!hits[0].snippet.is_empty(), "snippet must be filled");

    let none = store
        .search_notes("динозавр", &NoteFilter::default(), 20)
        .expect("search");
    assert!(none.is_empty());

    // Обновление тела убирает старый текст из индекса.
    let work = store
        .search_notes("встреча", &NoteFilter::default(), 20)
        .expect("search")
        .remove(0);
    store
        .update_note(
            &work.note.id,
            NotePatch {
                body: Some("совсем другой текст".into()),
                ..Default::default()
            },
        )
        .expect("update");
    assert!(store
        .search_notes("встреча", &NoteFilter::default(), 20)
        .expect("search")
        .is_empty());
    assert_eq!(
        store
            .search_notes("другой", &NoteFilter::default(), 20)
            .expect("search")
            .len(),
        1
    );
}

#[test]
fn search_respects_operators_and_scope() {
    let store = store();
    let pinned = store
        .create_note(note("Закреплённая", "общий текст"))
        .expect("pinned");
    store.set_note_pinned(&pinned.id, true).expect("pin");
    store
        .create_note(note("Обычная", "общий текст"))
        .expect("plain");

    let only_pinned = store
        .search_notes("общий is:pinned", &NoteFilter::default(), 20)
        .expect("search");
    assert_eq!(only_pinned.len(), 1);
    assert_eq!(only_pinned[0].note.title, "Закреплённая");

    let excluded = store
        .search_notes("общий -Закреплённая", &NoteFilter::default(), 20)
        .expect("search");
    assert_eq!(excluded.len(), 1);
    assert_eq!(excluded[0].note.title, "Обычная");

    let by_tag = store
        .search_notes("общий tag:несуществует", &NoteFilter::default(), 20)
        .expect("search");
    assert!(
        by_tag.is_empty(),
        "tag filter is applied on top of the text match"
    );
}

#[test]
fn unknown_operator_is_rejected_with_hint() {
    let store = store();
    store.create_note(note("Заметка", "текст")).expect("create");
    let error = store
        .search_notes("is:красиво", &NoteFilter::default(), 10)
        .expect_err("must fail");
    assert!(error.to_string().contains("неизвестный флаг"));
}

#[test]
fn trash_archive_and_scopes_are_isolated() {
    let store = store();
    let active = store.create_note(note("Активная", "а")).expect("active");
    let archived = store.create_note(note("Архивная", "б")).expect("archived");
    let trashed = store.create_note(note("В корзине", "в")).expect("trashed");
    store
        .set_note_archived(&archived.id, true)
        .expect("archive");
    store.delete_note(&trashed.id).expect("delete");

    let list = |scope| {
        store
            .list_notes(&NoteQuery {
                filter: NoteFilter {
                    scope: Some(scope),
                    ..Default::default()
                },
                ..Default::default()
            })
            .expect("list")
    };
    assert_eq!(list(Scope::Active).total, 1);
    assert_eq!(list(Scope::Archived).total, 1);
    assert_eq!(list(Scope::Trashed).total, 1);
    assert_eq!(list(Scope::ActiveAndArchived).total, 2);
    assert_eq!(list(Scope::Everything).total, 3);

    let restored = store.restore_note(&trashed.id).expect("restore");
    assert!(restored.deleted_at.is_none());
    assert_eq!(list(Scope::Active).total, 2);

    // Безвозвратное удаление чистит и версии, и связи.
    store.purge_note(&active.id).expect("purge");
    assert!(store.get_note(&active.id).is_err());
    assert_eq!(list(Scope::Everything).total, 2);
}

#[test]
fn empty_trash_removes_everything_deleted() {
    let store = store();
    for index in 0..3 {
        let created = store
            .create_note(note(&format!("Заметка {index}"), "текст"))
            .expect("create");
        store.delete_note(&created.id).expect("delete");
    }
    assert_eq!(store.empty_trash().expect("empty"), 3);
    assert_eq!(store.library_stats().expect("stats").trashed, 0);
}

#[test]
fn sorting_and_pagination_are_stable() {
    let store = store();
    for index in 0..5 {
        store
            .create_note(NoteDraft {
                pinned: Some(index == 0),
                ..note(&format!("Заметка {index}"), "текст")
            })
            .expect("create");
    }
    let first_page = store
        .list_notes(&NoteQuery {
            sort: SortOrder::UpdatedDesc,
            limit: 2,
            offset: 0,
            ..Default::default()
        })
        .expect("page");
    assert_eq!(first_page.items.len(), 2);
    assert_eq!(first_page.total, 5);
    assert!(first_page.has_more);
    assert!(first_page.items[0].pinned, "pinned notes come first");

    let second_page = store
        .list_notes(&NoteQuery {
            sort: SortOrder::UpdatedDesc,
            limit: 2,
            offset: 2,
            ..Default::default()
        })
        .expect("page");
    assert!(second_page.has_more);
    let ids: Vec<&str> = first_page
        .items
        .iter()
        .chain(second_page.items.iter())
        .map(|item| item.id.as_str())
        .collect();
    let unique: std::collections::HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(unique.len(), ids.len(), "pages must not overlap");

    let by_title = store
        .list_notes(&NoteQuery {
            sort: SortOrder::TitleAsc,
            limit: 10,
            ..Default::default()
        })
        .expect("list");
    let titles: Vec<&str> = by_title
        .items
        .iter()
        .map(|item| item.title.as_str())
        .collect();
    let mut sorted = titles.clone();
    sorted.sort();
    assert_eq!(titles, sorted);
}

#[test]
fn filters_by_tag_color_and_dates() {
    let store = store();
    store
        .create_note(NoteDraft {
            tags: vec!["важное".into()],
            color: Some("red".into()),
            ..note("Красная", "текст")
        })
        .expect("first");
    store
        .create_note(NoteDraft {
            tags: vec!["важное".into(), "позже".into()],
            color: Some("blue".into()),
            ..note("Синяя", "текст")
        })
        .expect("second");

    let tag = store.list_tags().expect("tags");
    let important = tag
        .iter()
        .find(|item| item.name == "важное")
        .expect("tag")
        .id
        .clone();

    let both = store
        .list_notes(&NoteQuery {
            filter: NoteFilter {
                tags: vec![important.clone()],
                ..Default::default()
            },
            ..Default::default()
        })
        .expect("list");
    assert_eq!(both.total, 2);

    let red = store
        .list_notes(&NoteQuery {
            filter: NoteFilter {
                tags: vec![important],
                color: Some("red".into()),
                ..Default::default()
            },
            ..Default::default()
        })
        .expect("list");
    assert_eq!(red.total, 1);
    assert_eq!(red.items[0].title, "Красная");

    let future = store
        .list_notes(&NoteQuery {
            filter: NoteFilter {
                updated_since: Some(bookmarks_core::time::Timestamp::from_millis(i64::MAX / 2)),
                ..Default::default()
            },
            ..Default::default()
        })
        .expect("list");
    assert_eq!(future.total, 0);
}

#[test]
fn duplicates_are_detected_by_content_hash() {
    let store = store();
    store
        .create_note(note("Копия 1", "одинаковый текст"))
        .expect("first");
    store
        .create_note(note("Копия 1", "одинаковый текст"))
        .expect("second");
    store
        .create_note(note("Другая", "уникальный текст"))
        .expect("third");

    let groups = store.find_duplicates().expect("duplicates");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].count, 2);
}

#[test]
fn version_history_is_saved_and_pruned() {
    let store = store();
    let created = store
        .create_note(note("Заметка", "версия 1"))
        .expect("create");
    for index in 2..6 {
        store
            .update_note(
                &created.id,
                NotePatch {
                    body: Some(format!("версия {index}")),
                    ..Default::default()
                },
            )
            .expect("update");
    }
    let summaries = store.list_versions(&created.id, 50).expect("versions");
    assert_eq!(summaries.len(), 4, "each content change saves one snapshot");
    assert!(summaries[0].created_at >= summaries[1].created_at);

    let restored = store
        .restore_version(summaries.last().expect("oldest").id)
        .expect("restore");
    assert_eq!(restored.body, "версия 1");

    let settings = bookmarks_core::domain::VersionSettings {
        policy: bookmarks_core::domain::VersionPolicy::KeepRecent,
        keep_count: 2,
        keep_days: 90,
    };
    let removed = store.prune_versions(&settings).expect("prune");
    assert!(removed > 0);
    assert!(
        store
            .list_versions(&created.id, 50)
            .expect("versions")
            .len()
            <= 2
    );
}

#[test]
fn integrity_check_and_index_rebuild_work() {
    let store = store();
    store
        .create_note(note("Заметка", "текст для поиска"))
        .expect("create");
    assert!(store.verify_integrity().expect("integrity").is_empty());

    let rebuilt = store.rebuild_search_index().expect("rebuild");
    assert_eq!(rebuilt, 1);
    assert_eq!(
        store
            .search_notes("поиска", &NoteFilter::default(), 10)
            .expect("search")
            .len(),
        1
    );

    let stats = store.storage_stats().expect("stats");
    assert_eq!(stats.notes, 1);
    assert!(stats.database_bytes > 0);
}

#[test]
fn file_store_survives_reopen() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("notes/мarginbook.db");

    let id = {
        let store = Store::open(&path).expect("open");
        store
            .create_note(note("Постоянная", "текст"))
            .expect("create")
            .id
    };
    // WAL-файлы создаются рядом с базой.
    let store = Store::open(&path).expect("reopen");
    let fetched = store.get_note(&id).expect("note persisted");
    assert_eq!(fetched.title, "Постоянная");
    assert_eq!(store.library_stats().expect("stats").notes, 1);
}
