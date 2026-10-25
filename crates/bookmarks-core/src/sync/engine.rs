//! Движок синхронизации: трёхстороннее слияние и разрешение конфликтов.
//!
//! Раскладка удалённого хранилища — по устройствам:
//!
//! ```text
//! devices/<device_id>.json          манифест устройства (отпечатки всех заметок)
//! devices/<device_id>/notes/xx/<id>.json   тела заметок, выгруженные этим устройством
//! ```
//!
//! Каждое устройство пишет ТОЛЬКО в своё поддерево. Это принципиально: при общем
//! файле заметки устройство, разрешившее конфликт первым, затирало бы версию
//! второго, и конфликт переставал бы быть виден — правка терялась молча.
//!
//! Удалённое состояние собирается слиянием манифестов всех остальных устройств:
//! для каждой заметки берётся самая свежая запись. Дальше работает классическая
//! трёхсторонняя схема «база / локальное / удалённое», где база — собственный
//! манифест устройства с прошлой синхронизации.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::db::{Store, SyncNote};
use crate::error::{Error, Result, SyncErrorCode};
use crate::ids::EntityId;
use crate::sync::manifest::{fingerprint, DeviceManifest, ManifestNote};
use crate::sync::remote::{Remote, RemoteError};
use crate::time::Timestamp;

/// Каталог манифестов и данных устройств.
pub const DEVICES_DIR: &str = "devices";

/// Base64-префикс шифрованного контейнера (магия `BKENC1`).
const ENCRYPTED_PREFIX: &str = "QktFTkMx";

/// Политика разрешения конфликтов.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    /// Побеждает самая свежая правка; проигравшая сохраняется в историю версий.
    #[default]
    KeepNewest,
    /// Локальная версия всегда важнее удалённой.
    KeepLocal,
    /// Удалённая версия всегда важнее локальной.
    KeepRemote,
    /// Проигравшая версия дополнительно сохраняется заметкой «… (конфликт)».
    DuplicateBoth,
}

/// Одно действие плана синхронизации.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncStep {
    /// Выгрузить локальную заметку.
    Upload,
    /// Скачать удалённую заметку.
    Download,
    /// Удалить локальную (удалена на другом устройстве).
    DeleteLocal,
    /// Опубликовать удаление (заметка удалена локально).
    DeleteRemote,
    /// Конфликт, разрешённый по политике.
    Conflict,
}

/// Элемент плана синхронизации.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStepDetail {
    /// Действие.
    pub step: SyncStep,
    /// Заметка.
    pub note_id: EntityId,
    /// Заголовок — для отчёта пользователю.
    pub title: String,
    /// Устройство, на котором возникло изменение.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// Как разрешается конфликт.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
}

/// План синхронизации — то, что будет сделано.
///
/// План строится до применения, поэтому его можно показать пользователю
/// («12 изменений, 1 конфликт») и применить осознанно.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPlan {
    /// Список действий.
    pub steps: Vec<SyncStepDetail>,
    /// Число выгрузок.
    pub uploads: u32,
    /// Число загрузок.
    pub downloads: u32,
    /// Число локальных удалений.
    pub local_deletions: u32,
    /// Число публикуемых удалений.
    pub remote_deletions: u32,
    /// Число конфликтов.
    pub conflicts: u32,
}

impl SyncPlan {
    /// Пуст ли план.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Общее число действий.
    pub fn total(&self) -> u32 {
        self.uploads
            + self.downloads
            + self.local_deletions
            + self.remote_deletions
            + self.conflicts
    }

    fn push(
        &mut self,
        step: SyncStep,
        note_id: &str,
        title: &str,
        device: Option<String>,
        resolution: Option<String>,
    ) {
        match step {
            SyncStep::Upload => self.uploads += 1,
            SyncStep::Download => self.downloads += 1,
            SyncStep::DeleteLocal => self.local_deletions += 1,
            SyncStep::DeleteRemote => self.remote_deletions += 1,
            SyncStep::Conflict => self.conflicts += 1,
        }
        self.steps.push(SyncStepDetail {
            step,
            note_id: note_id.to_owned(),
            title: title.to_owned(),
            device,
            resolution,
        });
    }
}

/// Результат выполненной синхронизации.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncOutcome {
    /// Выполненный план.
    pub plan: SyncPlan,
    /// Сколько заметок фактически выгружено.
    pub uploaded: u32,
    /// Сколько заметок фактически скачано.
    pub downloaded: u32,
    /// Сколько удалено локально.
    pub deleted_locally: u32,
    /// Сколько удалений опубликовано.
    pub deleted_remotely: u32,
    /// Сколько конфликтов разрешено.
    pub conflicts_resolved: u32,
    /// Сколько версий добавлено в историю.
    pub versions_saved: u32,
    /// Начало операции.
    pub started_at: Timestamp,
    /// Момент завершения.
    pub finished_at: Timestamp,
    /// Ошибки отдельных заметок: синхронизация продолжается, а не падает целиком.
    #[serde(default)]
    pub errors: Vec<String>,
}

/// Тело заметки в файле удалённого хранилища.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePayload {
    /// Идентификатор заметки.
    pub id: EntityId,
    /// Заголовок.
    pub title: String,
    /// Тело (или контейнер сейфа, если `encrypted`).
    pub body: String,
    /// Идентификатор папки на устройстве-авторе (информационно).
    pub folder_id: Option<EntityId>,
    /// Путь папки — по нему принимающая сторона воссоздаёт структуру.
    #[serde(default)]
    pub folder_path: String,
    /// Названия тегов.
    pub tags: Vec<String>,
    /// Закреплена.
    pub pinned: bool,
    /// В архиве.
    pub archived: bool,
    /// Цветовая метка.
    pub color: Option<String>,
    /// Зашифрована.
    pub encrypted: bool,
    /// Создана.
    pub created_at: Timestamp,
    /// Изменена.
    pub updated_at: Timestamp,
    /// Хеш содержимого.
    pub content_hash: String,
    /// Устройство-автор последней правки.
    pub device_id: EntityId,
    /// Имя устройства-автора.
    pub device_name: String,
    /// Версия формата данных.
    pub format_version: u32,
}

impl NotePayload {
    /// Проверяет принятый файл до записи в базу.
    pub fn validate(&self) -> Result<()> {
        if !crate::ids::is_valid_id(&self.id) {
            return Err(Error::sync(
                format!("в файле заметки некорректный идентификатор: {}", self.id),
                SyncErrorCode::RemoteFormat,
            ));
        }
        if self.format_version > crate::DATA_FORMAT_VERSION {
            return Err(Error::sync(
                format!(
                    "файл заметки создан версией формата {} (поддерживается {})",
                    self.format_version,
                    crate::DATA_FORMAT_VERSION
                ),
                SyncErrorCode::RemoteFormat,
            ));
        }
        if self.title.chars().count() > crate::domain::limits::MAX_TITLE_CHARS {
            return Err(Error::sync(
                "заголовок в принятом файле длиннее допустимого",
                SyncErrorCode::RemoteFormat,
            ));
        }
        Ok(())
    }

    /// Преобразует в запись, которую понимает хранилище.
    pub fn into_sync_note(self) -> SyncNote {
        SyncNote {
            id: self.id,
            title: self.title,
            body: self.body,
            folder_id: self.folder_id,
            folder_path: self.folder_path,
            tags: self.tags,
            pinned: self.pinned,
            archived: self.archived,
            color: self.color,
            encrypted: self.encrypted,
            created_at: self.created_at,
            updated_at: self.updated_at,
            deleted: false,
            content_hash: self.content_hash,
        }
    }
}

/// Запись удалённого состояния заметки: данные и кто их опубликовал.
#[derive(Debug, Clone)]
pub struct RemoteNote {
    /// Отпечаток из манифеста.
    pub note: ManifestNote,
    /// Идентификатор устройства-источника.
    pub device_id: EntityId,
    /// Имя устройства-источника (показывается в отчёте о конфликте).
    pub device_name: String,
}

/// Слияние манифестов других устройств в одно удалённое состояние.
#[derive(Debug, Clone, Default)]
pub struct RemoteState {
    notes: BTreeMap<EntityId, RemoteNote>,
}

impl RemoteState {
    /// Устройство, которое последним меняло заметку.
    pub fn owner(&self, id: &str) -> Option<String> {
        self.notes.get(id).map(|entry| entry.device_name.clone())
    }

    /// Запись удалённого состояния.
    pub fn get(&self, id: &str) -> Option<&RemoteNote> {
        self.notes.get(id)
    }

    /// Итератор по всем записям.
    pub fn iter(&self) -> impl Iterator<Item = (&EntityId, &RemoteNote)> {
        self.notes.iter()
    }

    /// Число записей (для тестов и отчётов).
    pub fn len(&self) -> usize {
        self.notes.len()
    }

    /// Пусто ли состояние.
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }
}

fn merge_remote_manifests(manifests: &[DeviceManifest], own_device: &str) -> RemoteState {
    let mut state = RemoteState::default();
    for manifest in manifests {
        if manifest.device_id == own_device {
            continue;
        }
        for (id, note) in &manifest.notes {
            let replace = match state.notes.get(id) {
                // При равных метках времени выбираем детерминированно — по
                // идентификатору устройства: иначе две машины могли бы прийти
                // к разным победителям и «пинать» заметку друг другу по кругу.
                Some(existing) => {
                    note.updated_at > existing.note.updated_at
                        || (note.updated_at == existing.note.updated_at
                            && manifest.device_id > existing.device_id)
                }
                None => true,
            };
            if replace {
                state.notes.insert(
                    id.clone(),
                    RemoteNote {
                        note: note.clone(),
                        device_id: manifest.device_id.clone(),
                        device_name: manifest.device_name.clone(),
                    },
                );
            }
        }
    }
    state
}

/// Движок синхронизации.
///
/// Состояния между запусками не хранит: вся правда о прошлом — в собственном
/// манифесте устройства, который читается из удалённого хранилища.
pub struct SyncEngine {
    store: Store,
    remote: Arc<dyn Remote>,
    device_id: EntityId,
    device_name: String,
    policy: ConflictPolicy,
    encryption_key: Option<Vec<u8>>,
}

impl SyncEngine {
    /// Создаёт движок.
    pub fn new(
        store: Store,
        remote: Arc<dyn Remote>,
        device_id: EntityId,
        device_name: String,
        policy: ConflictPolicy,
    ) -> Self {
        Self {
            store,
            remote,
            device_id,
            device_name,
            policy,
            encryption_key: None,
        }
    }

    /// Включает шифрование выгружаемых тел (E2EE).
    ///
    /// Без ключа в хранилище попадает открытый Markdown; с ключом — контейнер
    /// `BKENC1`, который не читается ни провайдером облака, ни другим
    /// устройством без того же ключа.
    pub fn with_encryption(mut self, key: Vec<u8>) -> Self {
        self.encryption_key = Some(key);
        self
    }

    /// Описание транспорта — для журнала и экрана настроек.
    pub fn remote_description(&self) -> String {
        self.remote.describe()
    }

    /// Идентификатор устройства.
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// Проверяет доступность хранилища (кнопка «Проверить соединение»).
    pub fn probe(&self) -> Result<()> {
        let probe_path = format!("{DEVICES_DIR}/.probe-{}", self.device_id);
        self.remote
            .write(&probe_path, b"ok")
            .map_err(map_remote_error)?;
        self.remote.delete(&probe_path).map_err(map_remote_error)
    }

    /// Устройства, опубликовавшие манифесты (кроме своего).
    pub fn known_devices(&self) -> Result<Vec<KnownDevice>> {
        Ok(self
            .read_remote_manifests()?
            .into_iter()
            .map(|manifest| {
                // Число заметок считаем до перемещения полей.
                let notes = manifest.active_note_count() as u32;
                KnownDevice {
                    device_id: manifest.device_id,
                    device_name: manifest.device_name,
                    generated_at: manifest.generated_at,
                    notes,
                }
            })
            .collect())
    }

    /// Строит план, ничего не меняя.
    pub fn plan(&self) -> Result<SyncPlan> {
        let base = self.read_own_manifest().unwrap_or_else(|_| {
            DeviceManifest::new(self.device_id.clone(), self.device_name.clone())
        });
        let local = self.build_local_manifest()?;
        let remote_state = merge_remote_manifests(&self.read_remote_manifests()?, &self.device_id);

        let mut plan = SyncPlan::default();
        let mut titles = self.title_map()?;

        let remote_only: Vec<(&EntityId, &RemoteNote)> = remote_state
            .iter()
            .filter(|(id, _)| !local.notes.contains_key(*id))
            .collect();
        for (id, note) in local
            .notes
            .iter()
            .map(|(id, value)| (id, Some(value)))
            .chain(remote_only.into_iter().map(|(id, _)| (id, None)))
        {
            let _ = note;
            let title = titles.remove(id).unwrap_or_default();
            let id: &EntityId = id;
            let base_note = base.notes.get(id);
            let remote_entry = remote_state.get(id);
            let remote_note = remote_entry.map(|entry| &entry.note);
            let local_note = local.notes.get(id);

            let local_changed = match (base_note, local_note) {
                (_, None) => false,
                (None, Some(_)) => true,
                (Some(previous), Some(current)) => {
                    DeviceManifest::fingerprint_of(previous) != current.fingerprint
                }
            };
            let remote_changed = match (base_note, remote_note) {
                (_, None) => false,
                (None, Some(value)) => !value.deleted,
                (Some(previous), Some(current)) => {
                    DeviceManifest::fingerprint_of(previous)
                        != DeviceManifest::fingerprint_of(current)
                }
            };

            match (local_changed, remote_changed) {
                (false, false) => {}
                (true, false) => match local_note {
                    Some(note) if note.deleted => {
                        plan.push(SyncStep::DeleteRemote, id, &title, None, None)
                    }
                    _ => plan.push(SyncStep::Upload, id, &title, None, None),
                },
                (false, true) => {
                    let remote_note = remote_note.expect("remote_changed implies presence");
                    if remote_note.deleted {
                        plan.push(
                            SyncStep::DeleteLocal,
                            id,
                            &title,
                            remote_state.owner(id),
                            None,
                        );
                    } else {
                        plan.push(SyncStep::Download, id, &title, remote_state.owner(id), None);
                    }
                }
                (true, true) => {
                    let remote_note = remote_note.expect("remote_changed implies presence");
                    let resolution = self.resolve_conflict(local_note, remote_note);
                    plan.push(
                        SyncStep::Conflict,
                        id,
                        &title,
                        remote_state.owner(id),
                        Some(resolution),
                    );
                }
            }
        }

        Ok(plan)
    }

    /// Выполняет синхронизацию: план → применение → публикация манифеста.
    pub fn run(&self) -> Result<SyncOutcome> {
        let started_at = Timestamp::now();
        let plan = self.plan()?;
        let mut outcome = SyncOutcome {
            plan: plan.clone(),
            started_at,
            ..Default::default()
        };

        for step in &plan.steps {
            let result = match step.step {
                SyncStep::Upload => self.upload_note(&step.note_id).map(|()| false),
                SyncStep::DeleteRemote => self.publish_deletion(&step.note_id).map(|()| false),
                SyncStep::Download => self.download_note(&step.note_id),
                SyncStep::DeleteLocal => self.delete_local(&step.note_id),
                SyncStep::Conflict => {
                    self.apply_conflict(&step.note_id, step.resolution.as_deref())
                }
            };
            match (&step.step, result) {
                (SyncStep::Upload, Ok(_)) => outcome.uploaded += 1,
                (SyncStep::DeleteRemote, Ok(_)) => outcome.deleted_remotely += 1,
                (SyncStep::Download, Ok(saved)) => {
                    outcome.downloaded += 1;
                    outcome.versions_saved += u32::from(saved);
                }
                (SyncStep::DeleteLocal, Ok(saved)) => {
                    outcome.deleted_locally += 1;
                    outcome.versions_saved += u32::from(saved);
                }
                (SyncStep::Conflict, Ok(saved)) => {
                    outcome.conflicts_resolved += 1;
                    outcome.versions_saved += u32::from(saved);
                }
                (_, Err(error)) => outcome.errors.push(format!("{}: {error}", step.note_id)),
            }
        }

        // Манифест публикуется и при частичных ошибках: иначе успешно
        // синхронизированные заметки на следующем шаге снова считались бы
        // изменёнными и гонялись по кругу.
        let manifest = self.build_local_manifest()?;
        self.write_own_manifest(&manifest)?;
        outcome.finished_at = Timestamp::now();
        Ok(outcome)
    }

    // -----------------------------------------------------------------------
    // Внутренние шаги
    // -----------------------------------------------------------------------

    fn build_local_manifest(&self) -> Result<DeviceManifest> {
        let mut manifest = DeviceManifest::new(self.device_id.clone(), self.device_name.clone());
        for note in self.store.sync_snapshot()? {
            manifest.notes.insert(
                note.id.clone(),
                ManifestNote {
                    fingerprint: fingerprint(&note.content_hash, note.deleted),
                    content_hash: note.content_hash.clone(),
                    updated_at: note.updated_at,
                    deleted: note.deleted,
                    encrypted: note.encrypted,
                    folder_id: note.folder_id.clone(),
                },
            );
        }
        Ok(manifest)
    }

    fn manifest_path(&self) -> String {
        format!("{DEVICES_DIR}/{}.json", self.device_id)
    }

    fn note_path_for(device_id: &str, note_id: &str) -> String {
        note_path(DEVICES_DIR, note_id, device_id)
    }

    fn read_own_manifest(&self) -> Result<DeviceManifest> {
        let raw = self
            .remote
            .read(&self.manifest_path())
            .map_err(map_remote_error)?;
        let text = String::from_utf8(raw).map_err(|_| {
            Error::sync(
                "манифест устройства не является текстом",
                SyncErrorCode::RemoteFormat,
            )
        })?;
        DeviceManifest::from_json(&text)
    }

    fn read_remote_manifests(&self) -> Result<Vec<DeviceManifest>> {
        let files = self.remote.list(DEVICES_DIR).map_err(map_remote_error)?;
        let mut manifests = Vec::new();
        for file in files {
            if !file.path.ends_with(".json") || file.path.contains(&self.device_id) {
                continue;
            }
            let raw = match self.remote.read(&file.path) {
                Ok(data) => data,
                // Файл мог исчезнуть между листингом и чтением — не повод
                // прерывать синхронизацию.
                Err(RemoteError::NotFound(_)) => continue,
                Err(error) => return Err(map_remote_error(error)),
            };
            let Ok(text) = String::from_utf8(raw) else {
                continue;
            };
            // Повреждённый или чужой манифест игнорируется: принимать на его
            // основе решения опасно (можно удалить чужие заметки).
            if let Ok(manifest) = DeviceManifest::from_json(&text) {
                manifests.push(manifest);
            }
        }
        Ok(manifests)
    }

    fn write_own_manifest(&self, manifest: &DeviceManifest) -> Result<()> {
        let json = manifest.to_json()?;
        self.remote
            .write(&self.manifest_path(), json.as_bytes())
            .map_err(map_remote_error)
    }

    fn title_map(&self) -> Result<BTreeMap<EntityId, String>> {
        let mut titles = BTreeMap::new();
        for note in self.store.sync_snapshot()? {
            titles.insert(note.id.clone(), note.title);
        }
        Ok(titles)
    }

    /// Выгружает заметку в поддерево своего устройства.
    fn upload_note(&self, note_id: &str) -> Result<()> {
        let Some(note) = self.store.get_sync_note(note_id)? else {
            return self.publish_deletion(note_id);
        };
        if note.deleted {
            return self.publish_deletion(note_id);
        }

        let payload = NotePayload {
            id: note.id.clone(),
            title: note.title.clone(),
            body: note.body.clone(),
            folder_id: note.folder_id.clone(),
            folder_path: note.folder_path.clone(),
            tags: note.tags.clone(),
            pinned: note.pinned,
            archived: note.archived,
            color: note.color.clone(),
            encrypted: note.encrypted,
            created_at: note.created_at,
            updated_at: note.updated_at,
            content_hash: note.content_hash.clone(),
            device_id: self.device_id.clone(),
            device_name: self.device_name.clone(),
            format_version: crate::DATA_FORMAT_VERSION,
        };
        let json = serde_json::to_vec_pretty(&payload)?;
        // Формат на диске всегда base64-контейнер — и для шифрованных, и для
        // открытых данных. Единый формат упрощает приём: вторая сторона не
        // должна угадывать, в каком виде записан файл.
        let encoded = match &self.encryption_key {
            Some(key) => crate::crypto::encrypt_container(key, &json)?,
            None => base64_encode(&json),
        };
        self.remote
            .write(
                &Self::note_path_for(&self.device_id, note_id),
                encoded.as_bytes(),
            )
            .map_err(map_remote_error)
    }

    /// Публикует удаление: файл убирается, а надгробие остаётся в манифесте.
    fn publish_deletion(&self, note_id: &str) -> Result<()> {
        self.remote
            .delete(&Self::note_path_for(&self.device_id, note_id))
            .map_err(map_remote_error)
    }

    /// Скачивает заметку; возвращает `true`, если сохранена версия.
    fn download_note(&self, note_id: &str) -> Result<bool> {
        let (payload, _owner) = self.fetch_payload(note_id)?;
        let mut saved_version = false;
        if let Some(existing) = self.store.get_sync_note(note_id)? {
            if existing.content_hash == payload.content_hash && !existing.deleted {
                return Ok(false);
            }
            saved_version =
                self.store
                    .save_remote_version(note_id, &existing.title, &existing.body)?;
        }
        self.store.apply_remote_note(&payload.into_sync_note())?;
        Ok(saved_version)
    }

    /// Применяет удалённое удаление; возвращает `true`, если сохранена версия.
    fn delete_local(&self, note_id: &str) -> Result<bool> {
        let Some(existing) = self.store.get_sync_note(note_id)? else {
            return Ok(false);
        };
        if existing.deleted {
            return Ok(false);
        }
        let saved = self
            .store
            .save_remote_version(note_id, &existing.title, &existing.body)?;
        self.store.delete_note(note_id)?;
        Ok(saved)
    }

    /// Разрешает конфликт по политике; возвращает `true`, если сохранена версия.
    fn apply_conflict(&self, note_id: &str, resolution: Option<&str>) -> Result<bool> {
        // Удалённого файла может не быть (другое устройство уже опубликовало
        // удаление) — тогда конфликт снимается удалением локальной копии.
        let fetched = match self.fetch_payload(note_id) {
            Ok((payload, owner)) => Some((payload, owner)),
            Err(Error::NotFound { .. }) => None,
            Err(error) => return Err(error),
        };
        let local = self.store.get_sync_note(note_id)?;

        let (remote, owner) = match fetched {
            Some(pair) => pair,
            // Удалённую версию уже не получить: конфликт снимается сам собой.
            None => return Ok(false),
        };
        let Some(local_note) = local else {
            self.store.apply_remote_note(&remote.into_sync_note())?;
            return Ok(false);
        };

        let keep_local = match resolution.unwrap_or("keep_newest") {
            "keep_local" => true,
            "keep_remote" => false,
            // keep_newest и duplicate_both: побеждает более свежая правка.
            _ => local_note.updated_at >= remote.updated_at,
        };

        let (winner, loser) = if keep_local {
            (local_note.clone(), remote.into_sync_note())
        } else {
            (remote.into_sync_note(), local_note.clone())
        };

        let saved = self
            .store
            .save_remote_version(note_id, &loser.title, &loser.body)?;
        if !keep_local {
            self.store.apply_remote_note(&winner)?;
        }
        if self.policy == ConflictPolicy::DuplicateBoth {
            self.store.create_conflict_copy(&loser, &owner)?;
        }
        // Победившая версия публикуется в своём поддереве: другие устройства
        // увидят её как самую свежую и придут к тому же выводу.
        self.upload_note(note_id)?;
        Ok(saved)
    }

    /// Читает файл заметки с любого устройства (ищет по всем манифестам).
    fn fetch_payload(&self, note_id: &str) -> Result<(NotePayload, String)> {
        let manifests = self.read_remote_manifests()?;
        // Кандидаты — устройства, у которых заметка не помечена удалённой;
        // перебираем от самого свежего к старым, чтобы взять актуальную версию.
        let mut candidates: Vec<(Timestamp, String, String)> = manifests
            .iter()
            .filter_map(|manifest| {
                manifest
                    .notes
                    .get(note_id)
                    .filter(|note| !note.deleted)
                    .map(|note| {
                        (
                            note.updated_at,
                            manifest.device_id.clone(),
                            manifest.device_name.clone(),
                        )
                    })
            })
            .collect();
        // Свежие первыми: при нескольких копиях берём последнюю правку.
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));

        let mut last_error: Option<Error> = None;
        for (_, device_id, device_name) in candidates {
            let path = Self::note_path_for(&device_id, note_id);
            match self.remote.read(&path) {
                Ok(raw) => match self.decode_payload(&raw) {
                    Ok(payload) => return Ok((payload, device_name)),
                    Err(error) => last_error = Some(error),
                },
                Err(RemoteError::NotFound(_)) => continue,
                Err(error) => return Err(map_remote_error(error)),
            }
        }

        // Файла нет ни у кого: либо заметка ещё не выгружена, либо удалена.
        Err(last_error.unwrap_or_else(|| Error::NotFound {
            entity: "remote file",
            id: Self::note_path_for(&self.device_id, note_id),
        }))
    }

    fn decode_payload(&self, raw: &[u8]) -> Result<NotePayload> {
        let text = String::from_utf8(raw.to_vec()).map_err(|_| {
            Error::sync(
                "файл заметки не является текстом",
                SyncErrorCode::RemoteFormat,
            )
        })?;
        // Формат файла определяется по содержимому, а не по наличию ключа:
        // зашифрованный контейнер начинается с base64-префикса магии `BKENC1`.
        // Иначе устройство с ключом не смогло бы принять открытую заметку.
        let json = if text.trim_start().starts_with(ENCRYPTED_PREFIX) {
            let key = self.encryption_key.as_ref().ok_or_else(|| {
                Error::sync(
                    "файл заметки зашифрован, а синхронизация запущена без ключа сейфа",
                    SyncErrorCode::RemoteFormat,
                )
            })?;
            crate::crypto::decrypt_container(key, &text)?
        } else {
            base64_decode(&text).ok_or_else(|| {
                Error::sync(
                    "файл заметки не является данными Bookmarks",
                    SyncErrorCode::RemoteFormat,
                )
            })?
        };
        let payload: NotePayload = serde_json::from_str(&json)?;
        payload.validate()?;
        Ok(payload)
    }

    /// Выбирает способ разрешения конфликта до применения — значение попадает в
    /// план, чтобы пользователь видел решение заранее.
    fn resolve_conflict(&self, local: Option<&ManifestNote>, remote: &ManifestNote) -> String {
        match self.policy {
            ConflictPolicy::KeepLocal => "keep_local".to_owned(),
            ConflictPolicy::KeepRemote => "keep_remote".to_owned(),
            ConflictPolicy::DuplicateBoth => "duplicate_both".to_owned(),
            ConflictPolicy::KeepNewest => match local {
                Some(local) if local.updated_at >= remote.updated_at => "keep_local".to_owned(),
                _ => "keep_remote".to_owned(),
            },
        }
    }

    /// Путь к файлу заметки своего устройства (для отладки и тестов).
    pub fn own_note_path_for(&self, note_id: &str) -> String {
        Self::note_path_for(&self.device_id, note_id)
    }
}

/// Устройство, найденное в удалённом хранилище.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownDevice {
    /// Идентификатор.
    pub device_id: EntityId,
    /// Имя.
    pub device_name: String,
    /// Момент публикации манифеста.
    pub generated_at: Timestamp,
    /// Число заметок в манифесте.
    pub notes: u32,
}

/// Путь файла заметки: `devices/<device>/notes/<xx>/<id>.json`.
///
/// Двухсимвольный префикс распределяет файлы по подкаталогам: в одной папке не
/// скапливаются десятки тысяч файлов, из-за чего листинг на сетевых дисках
/// становится заметно медленным.
fn note_path(devices_dir: &str, note_id: &str, device_id: &str) -> String {
    let prefix: String = note_id.chars().take(2).collect();
    format!("{devices_dir}/{device_id}/notes/{prefix}/{note_id}.json")
}

fn map_remote_error(error: RemoteError) -> Error {
    match error {
        RemoteError::NotFound(path) => Error::NotFound {
            entity: "remote file",
            id: path,
        },
        RemoteError::Offline(message) => Error::sync(message, SyncErrorCode::Offline),
        RemoteError::Auth(message) => Error::sync(message, SyncErrorCode::Auth),
        RemoteError::InvalidPath(path) => Error::sync(
            format!("недопустимый путь в хранилище: {path}"),
            SyncErrorCode::RemoteFormat,
        ),
        RemoteError::Io(message) => Error::sync(message, SyncErrorCode::Offline),
    }
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn base64_decode(text: &str) -> Option<String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(text.trim().as_bytes())
        .ok()?;
    String::from_utf8(bytes).ok()
}
