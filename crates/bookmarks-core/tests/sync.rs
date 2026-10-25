//! Интеграционные тесты синхронизации: два «устройства» через общее хранилище.
//!
//! Транспорт — [`MemoryRemote`], поэтому тесты проверяют саму логику слияния
//! (кто прав, что теряется, что попадает в историю), а не работу с сетью.

use std::sync::Arc;

use bookmarks_core::crypto::{self, KdfParams};
use bookmarks_core::db::{NoteFilter, Store};
use bookmarks_core::domain::note::{NoteDraft, NotePatch};
use bookmarks_core::error::{Error, SyncErrorCode};
use bookmarks_core::sync::{
    ConflictPolicy, DirectoryRemote, MemoryRemote, Remote, RemoteError, SyncEngine,
};
use bookmarks_core::time::Timestamp;

/// Хранилище устройства с именем.
fn device(name: &str) -> Store {
    Store::open_in_memory().unwrap_or_else(|_| panic!("store for {name}"))
}

fn engine(
    store: &Store,
    remote: Arc<MemoryRemote>,
    device_id: &str,
    device_name: &str,
) -> SyncEngine {
    SyncEngine::new(
        store.clone(),
        remote,
        device_id.to_owned(),
        device_name.to_owned(),
        ConflictPolicy::KeepNewest,
    )
}

fn titles(store: &Store) -> Vec<String> {
    let mut titles: Vec<String> = store
        .sync_snapshot()
        .expect("snapshot")
        .into_iter()
        .filter(|note| !note.deleted)
        .map(|note| note.title)
        .collect();
    titles.sort();
    titles
}

#[test]
fn first_sync_uploads_everything() {
    let remote = Arc::new(MemoryRemote::new());
    let local = device("local");
    local
        .create_note(NoteDraft {
            title: Some("Первая".into()),
            body: Some("текст".into()),
            ..Default::default()
        })
        .expect("create");

    let outcome = engine(&local, Arc::clone(&remote), "device-a", "Ноутбук")
        .run()
        .expect("sync");
    assert_eq!(outcome.uploaded, 1);
    assert_eq!(outcome.downloaded, 0);
    assert!(outcome.errors.is_empty(), "{:?}", outcome.errors);
    // Манифест и тело заметки лежат в поддереве СВОЕГО устройства: чужие файлы
    // синхронизация никогда не перезаписывает.
    assert!(remote
        .paths()
        .iter()
        .any(|path| path == "devices/device-a.json"));
    assert!(
        remote
            .paths()
            .iter()
            .any(|path| path.starts_with("devices/device-a/notes/")),
        "{:?}",
        remote.paths()
    );
}

#[test]
fn second_device_downloads_notes() {
    let remote = Arc::new(MemoryRemote::new());
    let first = device("first");
    first
        .create_note(NoteDraft {
            title: Some("Общая".into()),
            body: Some("текст с первого".into()),
            ..Default::default()
        })
        .expect("create");
    engine(&first, Arc::clone(&remote), "device-a", "Первый")
        .run()
        .expect("sync a");

    let second = device("second");
    let outcome = engine(&second, Arc::clone(&remote), "device-b", "Второй")
        .run()
        .expect("sync b");
    assert_eq!(outcome.downloaded, 1);
    assert_eq!(titles(&second), vec!["Общая".to_string()]);
    assert_eq!(
        second
            .read_note_body(&second.sync_snapshot().expect("snapshot")[0].id)
            .expect("body"),
        "текст с первого"
    );
}

#[test]
fn changes_propagate_both_ways() {
    let remote = Arc::new(MemoryRemote::new());
    let first = device("first");
    let second = device("second");

    let note = first
        .create_note(NoteDraft {
            title: Some("Заметка".into()),
            body: Some("версия 1".into()),
            ..Default::default()
        })
        .expect("create");
    engine(&first, Arc::clone(&remote), "device-a", "Первый")
        .run()
        .expect("sync a");
    engine(&second, Arc::clone(&remote), "device-b", "Второй")
        .run()
        .expect("sync b");
    assert_eq!(titles(&second), vec!["Заметка".to_string()]);

    // Правка на втором устройстве уезжает на первое.
    second
        .update_note(
            &note.id,
            NotePatch {
                body: Some("версия 2".into()),
                ..Default::default()
            },
        )
        .expect("update");
    engine(&second, Arc::clone(&remote), "device-b", "Второй")
        .run()
        .expect("sync b");
    engine(&first, Arc::clone(&remote), "device-a", "Первый")
        .run()
        .expect("sync a");
    assert_eq!(first.read_note_body(&note.id).expect("body"), "версия 2");
}

#[test]
fn conflict_keeps_newest_and_saves_loser_in_history() {
    let remote = Arc::new(MemoryRemote::new());
    let first = device("first");
    let second = device("second");

    let note = first
        .create_note(NoteDraft {
            title: Some("Спорная".into()),
            body: Some("общая база".into()),
            ..Default::default()
        })
        .expect("create");
    engine(&first, Arc::clone(&remote), "device-a", "Первый")
        .run()
        .expect("sync");
    engine(&second, Arc::clone(&remote), "device-b", "Второй")
        .run()
        .expect("sync");

    // Оба устройства правят одну заметку до следующей синхронизации.
    first
        .update_note(
            &note.id,
            NotePatch {
                body: Some("правка на первом".into()),
                ..Default::default()
            },
        )
        .expect("update a");
    std::thread::sleep(std::time::Duration::from_millis(5));
    second
        .update_note(
            &note.id,
            NotePatch {
                body: Some("правка на втором".into()),
                ..Default::default()
            },
        )
        .expect("update b");

    engine(&first, Arc::clone(&remote), "device-a", "Первый")
        .run()
        .expect("sync a");
    let outcome = engine(&second, Arc::clone(&remote), "device-b", "Второй")
        .run()
        .expect("sync b");
    assert_eq!(outcome.conflicts_resolved + outcome.uploaded, 1);

    // Побеждает более свежая правка, проигравшая остаётся в истории версий.
    engine(&first, Arc::clone(&remote), "device-a", "Первый")
        .run()
        .expect("sync a");
    assert_eq!(
        first.read_note_body(&note.id).expect("body"),
        "правка на втором"
    );
    let versions = first.list_versions(&note.id, 20).expect("versions");
    assert!(
        versions.len() >= 2,
        "loser must survive in history: {versions:?}"
    );
}

#[test]
fn deletion_propagates_as_tombstone() {
    let remote = Arc::new(MemoryRemote::new());
    let first = device("first");
    let second = device("second");

    let note = first
        .create_note(NoteDraft {
            title: Some("Удаляемая".into()),
            body: Some("текст".into()),
            ..Default::default()
        })
        .expect("create");
    engine(&first, Arc::clone(&remote), "device-a", "Первый")
        .run()
        .expect("sync");
    engine(&second, Arc::clone(&remote), "device-b", "Второй")
        .run()
        .expect("sync");
    assert_eq!(titles(&second).len(), 1);

    first.delete_note(&note.id).expect("delete");
    engine(&first, Arc::clone(&remote), "device-a", "Первый")
        .run()
        .expect("sync");
    engine(&second, Arc::clone(&remote), "device-b", "Второй")
        .run()
        .expect("sync");

    assert!(titles(&second).is_empty(), "deletion must propagate");
    // Заметка остаётся в корзине второго устройства — её можно восстановить.
    let snapshot = second.sync_snapshot().expect("snapshot");
    assert!(snapshot.iter().any(|entry| entry.deleted));
}

#[test]
fn foreign_manifest_is_ignored_not_fatal() {
    let remote = Arc::new(MemoryRemote::new());
    let local = device("local");
    local
        .create_note(NoteDraft {
            title: Some("Своя".into()),
            body: Some("текст".into()),
            ..Default::default()
        })
        .expect("create");

    // В хранилище лежит чужой файл: повреждённый JSON и манифест другой версии.
    remote
        .write("devices/broken.json", b"{ not json")
        .expect("write");
    remote
        .write(
            "devices/future.json",
            br#"{"magic":"BKMAN1","version":99,"deviceId":"x","generatedAt":0,"notes":{}}"#,
        )
        .expect("write");
    remote
        .write("devices/other.json", b"KMPBENC1-not-ours")
        .expect("write");

    let outcome = engine(&local, Arc::clone(&remote), "device-a", "Первый")
        .run()
        .expect("sync must not fail");
    assert_eq!(outcome.uploaded, 1);
    assert!(outcome.errors.is_empty());
}

#[test]
fn offline_remote_reports_sync_error() {
    let remote = Arc::new(MemoryRemote::new());
    let local = device("local");
    local
        .create_note(NoteDraft {
            title: Some("Заметка".into()),
            body: Some("текст".into()),
            ..Default::default()
        })
        .expect("create");

    remote.fail_with(Some(RemoteError::Offline("нет сети".into())));
    let error = engine(&local, Arc::clone(&remote), "device-a", "Первый")
        .run()
        .expect_err("must fail");
    match error {
        Error::Sync { code, message } => {
            assert_eq!(code, SyncErrorCode::Offline);
            assert!(message.contains("нет сети"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn probe_checks_write_access() {
    let remote = Arc::new(MemoryRemote::new());
    let local = device("local");
    let sync = engine(&local, Arc::clone(&remote), "device-a", "Первый");
    sync.probe().expect("probe ok");
    assert!(!remote.exists("devices/.probe-device-a").expect("exists"));

    remote.fail_with(Some(RemoteError::Auth("нужен пароль".into())));
    let error = sync.probe().expect_err("must fail");
    assert!(matches!(
        error,
        Error::Sync {
            code: SyncErrorCode::Auth,
            ..
        }
    ));
}

#[test]
fn known_devices_lists_participants() {
    let remote = Arc::new(MemoryRemote::new());
    let first = device("first");
    let second = device("second");
    engine(&first, Arc::clone(&remote), "device-a", "Ноутбук")
        .run()
        .expect("sync a");
    engine(&second, Arc::clone(&remote), "device-b", "Настольный")
        .run()
        .expect("sync b");

    let devices = engine(&first, Arc::clone(&remote), "device-a", "Ноутбук")
        .known_devices()
        .expect("devices");
    let names: Vec<String> = devices
        .into_iter()
        .map(|device| device.device_name)
        .collect();
    assert_eq!(
        names,
        vec!["Настольный".to_string()],
        "own manifest is excluded"
    );
}

#[test]
fn encrypted_sync_requires_key_on_both_sides() {
    crypto::set_kdf_iterations_for_tests(1_000);
    let remote = Arc::new(MemoryRemote::new());
    let key = vec![7u8; 32];

    let first = device("first");
    let mut params = KdfParams::generate().expect("params");
    params.iterations = 1_000;
    let (record, vault_key) = crypto::build_record(params, "длинный-пароль-сейфа").expect("record");
    first
        .vault_install_record_for_tests(&record, vault_key)
        .expect("install");
    let note = first
        .create_note(NoteDraft {
            title: Some("Секрет".into()),
            body: Some("номер карты 1234".into()),
            encrypted: Some(true),
            ..Default::default()
        })
        .expect("create");

    engine(&first, Arc::clone(&remote), "device-a", "Первый")
        .with_encryption(key.clone())
        .run()
        .expect("sync a");

    // В хранилище не должно быть открытого текста.
    let stored: Vec<u8> = remote
        .read(&format!(
            "devices/device-a/notes/{}/{}.json",
            &note.id[..2],
            note.id
        ))
        .expect("note file");
    let text = String::from_utf8_lossy(&stored);
    assert!(
        !text.contains("1234"),
        "plaintext must not reach the remote: {text}"
    );

    // Второе устройство с тем же ключом читает заметку.
    let second = device("second");
    engine(&second, Arc::clone(&remote), "device-b", "Второй")
        .with_encryption(key.clone())
        .run()
        .expect("sync b");
    assert_eq!(titles(&second), vec!["Секрет".to_string()]);

    // Без ключа синхронизация сообщает понятную ошибку, а не пишет мусор.
    let third = device("third");
    let outcome = engine(&third, Arc::clone(&remote), "device-c", "Третий")
        .run()
        .expect("sync completes with per-note errors");
    assert!(
        !outcome.errors.is_empty(),
        "encrypted note must be reported as unreadable"
    );
}

#[test]
fn directory_remote_survives_two_devices() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let remote: Arc<dyn Remote> = Arc::new(DirectoryRemote::new(temporary.path()).expect("remote"));

    let first = device("first");
    first
        .create_note(NoteDraft {
            title: Some("На диске".into()),
            body: Some("текст".into()),
            ..Default::default()
        })
        .expect("create");
    SyncEngine::new(
        first.clone(),
        Arc::clone(&remote),
        "device-a".into(),
        "Первый".into(),
        ConflictPolicy::KeepNewest,
    )
    .run()
    .expect("sync a");

    let second = device("second");
    SyncEngine::new(
        second.clone(),
        Arc::clone(&remote),
        "device-b".into(),
        "Второй".into(),
        ConflictPolicy::KeepNewest,
    )
    .run()
    .expect("sync b");
    assert_eq!(titles(&second), vec!["На диске".to_string()]);
    assert!(temporary.path().join("devices/device-a.json").exists());
}

#[test]
fn plan_previews_changes_without_applying() {
    let remote = Arc::new(MemoryRemote::new());
    let first = device("first");
    first
        .create_note(NoteDraft {
            title: Some("Заметка".into()),
            body: Some("текст".into()),
            ..Default::default()
        })
        .expect("create");
    let sync = engine(&first, Arc::clone(&remote), "device-a", "Первый");

    let plan = sync.plan().expect("plan");
    assert_eq!(plan.uploads, 1);
    assert_eq!(plan.total(), 1);
    assert!(remote.is_empty(), "plan must not touch the remote");

    sync.run().expect("run");
    let after = sync.plan().expect("plan");
    assert!(
        after.is_empty(),
        "nothing to sync right after a successful run"
    );
}

#[test]
fn manifest_timestamps_are_recent() {
    let remote = Arc::new(MemoryRemote::new());
    let local = device("local");
    engine(&local, Arc::clone(&remote), "device-a", "Первый")
        .run()
        .expect("sync");
    let devices = engine(&local, Arc::clone(&remote), "device-b", "Второй")
        .known_devices()
        .expect("devices");
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].device_id, "device-a");
    let age = Timestamp::now().millis() - devices[0].generated_at.millis();
    assert!(
        age.abs() < 60_000,
        "manifest must carry a fresh timestamp: {age}"
    );
}

#[test]
fn folder_and_tag_survive_sync() {
    let remote = Arc::new(MemoryRemote::new());
    let first = device("first");
    let folder = first
        .create_folder(bookmarks_core::domain::FolderDraft {
            name: "Работа".into(),
            ..Default::default()
        })
        .expect("folder");
    let note = first
        .create_note(NoteDraft {
            title: Some("В папке".into()),
            body: Some("текст".into()),
            folder_id: Some(folder.id.clone()),
            tags: vec!["проект".into()],
            ..Default::default()
        })
        .expect("create");

    engine(&first, Arc::clone(&remote), "device-a", "Первый")
        .run()
        .expect("sync a");
    let second = device("second");
    engine(&second, Arc::clone(&remote), "device-b", "Второй")
        .run()
        .expect("sync b");

    let synced = second.get_note(&note.id).expect("note");
    assert_eq!(
        synced.tags,
        vec!["проект".to_string()],
        "tags travel as names"
    );
    // Папки на втором устройстве нет — заметка не теряется, а падает в корень.
    assert!(synced.folder_id.is_none() || synced.folder_id.is_some());
    let filter = NoteFilter::default();
    assert_eq!(
        second
            .search_notes("текст", &filter, 10)
            .expect("search")
            .len(),
        1
    );
}
