//! Интеграционные тесты сейфа: шифрование заметок, блокировка, смена пароля.

use bookmarks_core::crypto::{self, KdfParams, VaultRecord};
use bookmarks_core::db::{NoteFilter, Store};
use bookmarks_core::domain::note::{NoteDraft, NotePatch};
use bookmarks_core::error::{Error, VaultErrorCode};

/// Хранилище с сейфом на уменьшенных итерациях KDF.
///
/// Штатные 210 000 итераций PBKDF2 сделали бы прогон тестов неприлично долгим;
/// логика от числа итераций не зависит, поэтому тесты подставляют быстрые
/// параметры через `vault_apply_test_params`.
fn vault_store(password: &str) -> Store {
    crypto::set_kdf_iterations_for_tests(1_000);
    let store = Store::open_in_memory().expect("store");
    let mut params = KdfParams::generate().expect("params");
    params.iterations = 1_000;
    let (record, key) = crypto::build_record(params, password).expect("record");
    store
        .vault_install_record_for_tests(&record, key)
        .expect("install vault");
    store
}

#[test]
fn encrypted_note_body_is_not_stored_in_plain() {
    let store = vault_store("достаточно-длинный-пароль");
    let created = store
        .create_note(NoteDraft {
            title: Some("Сейф".into()),
            body: Some("номер карты 1234".into()),
            encrypted: Some(true),
            ..Default::default()
        })
        .expect("create");
    assert!(created.encrypted);

    // В базе лежит контейнер, а не текст.
    let stored = store.stored_body_for_tests(&created.id).expect("stored");
    assert!(
        !stored.contains("1234"),
        "plaintext must not be stored: {stored}"
    );
    assert!(stored.starts_with("QktFTkMx"), "base64 of BKENC1 magic");

    // Приложение при этом читает открытый текст.
    assert_eq!(
        store.read_note_body(&created.id).expect("body"),
        "номер карты 1234"
    );
}

#[test]
fn encrypted_body_stays_out_of_search_index() {
    let store = vault_store("достаточно-длинный-пароль");
    store
        .create_note(NoteDraft {
            title: Some("Секретный план".into()),
            body: Some("пароль-от-сейфа-12345".into()),
            encrypted: Some(true),
            ..Default::default()
        })
        .expect("create encrypted");
    store
        .create_note(NoteDraft {
            title: Some("Обычная".into()),
            body: Some("открытый текст".into()),
            ..Default::default()
        })
        .expect("create plain");

    let hits = store
        .search_notes("пароль", &NoteFilter::default(), 20)
        .expect("search");
    assert!(hits.is_empty(), "encrypted body must not be searchable");

    // Заголовок остаётся в индексе: заметку должно быть видно в списке.
    let by_title = store
        .search_notes("Секретный", &NoteFilter::default(), 20)
        .expect("search");
    assert_eq!(by_title.len(), 1);

    // После вывода из сейфа тело снова ищется.
    let id = by_title[0].note.id.clone();
    store.decrypt_note(&id).expect("decrypt");
    assert_eq!(
        store
            .search_notes("пароль", &NoteFilter::default(), 20)
            .expect("search")
            .len(),
        1
    );
}

#[test]
fn locked_vault_blocks_reading_and_writing() {
    let store = vault_store("достаточно-длинный-пароль");
    let created = store
        .create_note(NoteDraft {
            title: Some("Сейф".into()),
            body: Some("текст".into()),
            encrypted: Some(true),
            ..Default::default()
        })
        .expect("create");

    store.vault_lock();
    assert!(!store.vault_is_unlocked().expect("state"));

    let error = store.read_note_body(&created.id).expect_err("must fail");
    match error {
        Error::Vault { code, .. } => assert_eq!(code, VaultErrorCode::BadPassword),
        other => panic!("unexpected error: {other:?}"),
    }
    let update = store.update_note(
        &created.id,
        NotePatch {
            body: Some("новая версия".into()),
            ..Default::default()
        },
    );
    assert!(
        matches!(update, Err(Error::Vault { .. })),
        "write must fail while locked"
    );

    // Разблокировка возвращает доступ.
    store
        .vault_unlock("достаточно-длинный-пароль")
        .expect("unlock");
    assert!(store.vault_is_unlocked().expect("state"));
    assert_eq!(store.read_note_body(&created.id).expect("body"), "текст");
}

#[test]
fn wrong_password_is_rejected() {
    let store = vault_store("достаточно-длинный-пароль");
    let error = store
        .vault_unlock("совершенно-другой")
        .expect_err("must fail");
    match error {
        Error::Vault { code, .. } => assert_eq!(code, VaultErrorCode::BadPassword),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn encrypted_flag_requires_initialized_vault() {
    let store = Store::open_in_memory().expect("store");
    let error = store
        .create_note(NoteDraft {
            encrypted: Some(true),
            ..Default::default()
        })
        .expect_err("must fail");
    match error {
        Error::Vault { code, .. } => assert_eq!(code, VaultErrorCode::NotInitialized),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn moving_note_into_vault_and_back_keeps_text() {
    let store = vault_store("достаточно-длинный-пароль");
    let created = store
        .create_note(NoteDraft {
            title: Some("Заметка".into()),
            body: Some("важный текст".into()),
            ..Default::default()
        })
        .expect("create");
    assert!(!created.encrypted);

    store.encrypt_note(&created.id).expect("encrypt");
    let encrypted = store.get_note(&created.id).expect("note");
    assert!(encrypted.encrypted);
    assert_eq!(
        store.read_note_body(&created.id).expect("body"),
        "важный текст"
    );

    store.decrypt_note(&created.id).expect("decrypt");
    let plain = store.get_note(&created.id).expect("note");
    assert!(!plain.encrypted);
    assert_eq!(plain.body, "важный текст");
}

#[test]
fn changing_password_reencrypts_everything() {
    let store = vault_store("первый-длинный-пароль");
    let first = store
        .create_note(NoteDraft {
            title: Some("Первая".into()),
            body: Some("текст один".into()),
            encrypted: Some(true),
            ..Default::default()
        })
        .expect("create");
    store
        .create_note(NoteDraft {
            title: Some("Вторая".into()),
            body: Some("текст два".into()),
            encrypted: Some(true),
            ..Default::default()
        })
        .expect("create");

    let reencrypted = store
        .vault_change_password("первый-длинный-пароль", "второй-длинный-пароль")
        .expect("change");
    assert_eq!(reencrypted, 2);

    assert_eq!(store.read_note_body(&first.id).expect("body"), "текст один");
    assert!(store.vault_unlock("второй-длинный-пароль").is_ok());
    assert!(store.vault_unlock("первый-длинный-пароль").is_err());

    // Смена пароля с неверным текущим не проходит.
    assert!(store
        .vault_change_password("неверный-старый", "третий-длинный")
        .is_err());
}

#[test]
fn disabling_vault_decrypts_notes() {
    let store = vault_store("достаточно-длинный-пароль");
    let created = store
        .create_note(NoteDraft {
            title: Some("Сейф".into()),
            body: Some("текст".into()),
            encrypted: Some(true),
            ..Default::default()
        })
        .expect("create");

    let decrypted = store
        .vault_disable("достаточно-длинный-пароль")
        .expect("disable");
    assert_eq!(decrypted, 1);
    assert!(store.vault_record().expect("record").is_none());

    let note = store.get_note(&created.id).expect("note");
    assert!(!note.encrypted);
    assert_eq!(note.body, "текст", "body must be readable without a vault");
}

#[test]
fn vault_survives_reopen_but_stays_locked() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("vault.db");

    let id = {
        let mut params = KdfParams::generate().expect("params");
        params.iterations = 1_000;
        let store = Store::open(&path).expect("open");
        let (record, key) = crypto::build_record(params, "постоянный-пароль").expect("record");
        store
            .vault_install_record_for_tests(&record, key)
            .expect("install");
        store
            .create_note(NoteDraft {
                title: Some("Сейф".into()),
                body: Some("секрет".into()),
                encrypted: Some(true),
                ..Default::default()
            })
            .expect("create")
            .id
    };

    let reopened = Store::open(&path).expect("reopen");
    assert!(reopened.vault_record().expect("record").is_some());
    assert!(!reopened.vault_is_unlocked().expect("locked after restart"));
    assert!(matches!(
        reopened.read_note_body(&id),
        Err(Error::Vault { .. })
    ));
    reopened.vault_unlock("постоянный-пароль").expect("unlock");
    assert_eq!(reopened.read_note_body(&id).expect("body"), "секрет");
}

#[test]
fn foreign_vault_record_is_rejected() {
    crypto::set_kdf_iterations_for_tests(1_000);
    let store = Store::open_in_memory().expect("store");
    let params = KdfParams::generate().expect("params");
    let record = VaultRecord::from_parts(params.to_json().expect("json"), "abcd".repeat(16))
        .expect("record");
    store
        .vault_install_record_for_tests(&record, vec![0u8; 32])
        .expect("install");
    // Верификатор не сходится ни с одним паролем.
    assert!(matches!(
        store.vault_unlock("какой-то-пароль"),
        Err(Error::Vault {
            code: VaultErrorCode::BadPassword,
            ..
        })
    ));
}
