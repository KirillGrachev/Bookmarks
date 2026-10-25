use bookmarks_core::crypto;
use bookmarks_core::db::Store;
use bookmarks_core::domain::note::NoteDraft;
use bookmarks_core::sync::{ConflictPolicy, MemoryRemote, SyncEngine};
use std::sync::Arc;

fn eng(store: &Store, remote: Arc<MemoryRemote>, id: &str, name: &str) -> SyncEngine {
    SyncEngine::new(
        store.clone(),
        remote,
        id.into(),
        name.into(),
        ConflictPolicy::KeepNewest,
    )
}

fn main() {
    crypto::set_kdf_iterations_for_tests(1_000);
    let remote = Arc::new(MemoryRemote::new());
    let key = vec![7u8; 32];

    let first = Store::open_in_memory().unwrap();
    let (record, vault_key) = crypto::initialize("длинный-пароль-сейфа").unwrap();
    first
        .vault_install_record_for_tests(&record, vault_key)
        .unwrap();
    let note = first
        .create_note(NoteDraft {
            title: Some("Секрет".into()),
            body: Some("номер карты 1234".into()),
            encrypted: Some(true),
            ..Default::default()
        })
        .expect("create");
    println!("created encrypted={}", note.encrypted);
    let out = eng(&first, Arc::clone(&remote), "device-a", "Первый")
        .with_encryption(key.clone())
        .run()
        .expect("sync a");
    println!("A: uploaded={} errors={:?}", out.uploaded, out.errors);
    println!("remote files: {:?}", remote.paths());

    let second = Store::open_in_memory().unwrap();
    let out = eng(&second, Arc::clone(&remote), "device-b", "Второй")
        .with_encryption(key.clone())
        .run()
        .expect("sync b");
    println!(
        "B: plan={:?} downloaded={} errors={:?}",
        out.plan
            .steps
            .iter()
            .map(|s| format!("{:?}", s.step))
            .collect::<Vec<_>>(),
        out.downloaded,
        out.errors
    );
    println!(
        "B snapshot: {:?}",
        second
            .sync_snapshot()
            .unwrap()
            .iter()
            .map(|n| (n.title.clone(), n.encrypted))
            .collect::<Vec<_>>()
    );

    // ---- conflict ----
    let remote2 = Arc::new(MemoryRemote::new());
    let a = Store::open_in_memory().unwrap();
    let b = Store::open_in_memory().unwrap();
    let n = a
        .create_note(NoteDraft {
            title: Some("Спорная".into()),
            body: Some("общая база".into()),
            ..Default::default()
        })
        .unwrap();
    eng(&a, Arc::clone(&remote2), "device-a", "Первый")
        .run()
        .unwrap();
    eng(&b, Arc::clone(&remote2), "device-b", "Второй")
        .run()
        .unwrap();
    a.update_note(
        &n.id,
        bookmarks_core::domain::note::NotePatch {
            body: Some("правка на первом".into()),
            ..Default::default()
        },
    )
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(10));
    b.update_note(
        &n.id,
        bookmarks_core::domain::note::NotePatch {
            body: Some("правка на втором".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let oa = eng(&a, Arc::clone(&remote2), "device-a", "Первый")
        .run()
        .unwrap();
    println!(
        "A2 plan={:?} conflicts={}",
        oa.plan
            .steps
            .iter()
            .map(|s| format!("{:?}/{}", s.step, s.resolution.clone().unwrap_or_default()))
            .collect::<Vec<_>>(),
        oa.conflicts_resolved
    );
    let ob = eng(&b, Arc::clone(&remote2), "device-b", "Второй")
        .run()
        .unwrap();
    println!(
        "B2 plan={:?} uploaded={}",
        ob.plan
            .steps
            .iter()
            .map(|s| format!("{:?}/{}", s.step, s.resolution.clone().unwrap_or_default()))
            .collect::<Vec<_>>(),
        ob.uploaded
    );
    let oc = eng(&a, Arc::clone(&remote2), "device-a", "Первый")
        .run()
        .unwrap();
    println!(
        "A3 plan={:?} conflicts={} errors={:?}",
        oc.plan
            .steps
            .iter()
            .map(|s| format!("{:?}/{}", s.step, s.resolution.clone().unwrap_or_default()))
            .collect::<Vec<_>>(),
        oc.conflicts_resolved,
        oc.errors
    );
    println!("A body = {:?}", a.read_note_body(&n.id).unwrap());
    println!("B body = {:?}", b.read_note_body(&n.id).unwrap());
}
