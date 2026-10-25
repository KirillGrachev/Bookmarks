//! Криптография сейфа.
//!
//! Формат контейнера `BKENC1` (собственный, версионируемый — по образцу
//! контейнера справочника, где любой чужой файл однозначно отвергается):
//!
//! ```text
//! "BKENC1" (6 байт) | version: u8 | salt: 16 | nonce: 12 | ciphertext+tag
//! ```
//!
//! Контейнер сериализуется в base64 и хранится в колонке `notes.body`.
//!
//! Вывод ключа: PBKDF2-HMAC-SHA256 (210 000 итераций — рекомендация OWASP на
//! момент написания) даёт мастер-ключ, из которого blake3 с доменным
//! разделением получает два независимых ключа:
//! - ключ шифрования (AES-256-GCM);
//! - ключ верификатора — его хеш сохраняется в базе и позволяет проверить
//!   пароль, не храня ни пароля, ни обратимой его производной.
//!
//! Соль у каждого контейнера своя: две одинаковые заметки дают разные
//! шифртексты, а перешифровка при смене пароля не переиспользует nonce.

use std::sync::Mutex;

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use hmac::Hmac;
use pbkdf2::pbkdf2;
use rand::RngCore;
use sha2::Sha256;

use crate::error::{Error, Result, VaultErrorCode};

/// Магический префикс контейнера.
pub const MAGIC: &[u8; 6] = b"BKENC1";

/// Текущая версия формата контейнера.
pub const CONTAINER_VERSION: u8 = 1;

/// Длина соли, байт.
pub const SALT_LEN: usize = 16;

/// Длина nonce AES-GCM, байт.
pub const NONCE_LEN: usize = 12;

/// Длина ключа, байт.
pub const KEY_LEN: usize = 32;

/// Число итераций PBKDF2.
pub const PBKDF2_ITERATIONS: u32 = 210_000;

/// Фактическое число итераций.
///
/// Отдельная переменная, а не константа, нужна только тестам: 210 000 итераций
/// PBKDF2 занимают ~0.1 с, и десятки тестов с сейфом шли бы секундами. На
/// поведение приложения значение не влияет — продакшен всегда использует
/// [`PBKDF2_ITERATIONS`].
static KDF_ITERATIONS: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(PBKDF2_ITERATIONS);

/// Текущее число итераций вывода ключа.
pub fn kdf_iterations() -> u32 {
    KDF_ITERATIONS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Служебный переключатель числа итераций для тестов.
pub fn set_kdf_iterations_for_tests(iterations: u32) {
    KDF_ITERATIONS.store(iterations.max(1), std::sync::atomic::Ordering::Relaxed);
}

/// Минимальная длина пароля сейфа, в символах.
pub const MIN_PASSWORD_CHARS: usize = 8;

/// Максимальная длина пароля сейфа, в символах.
pub const MAX_PASSWORD_CHARS: usize = 256;

/// Параметры вывода ключа, которые нужно знать для расшифровки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KdfParams {
    /// Алгоритм вывода ключа.
    pub algorithm: String,
    /// Число итераций.
    pub iterations: u32,
    /// Соль в base64.
    pub salt: String,
    /// Версия формата контейнера.
    pub version: u8,
}

impl KdfParams {
    /// Случайные параметры с новой солью.
    pub fn generate() -> Result<Self> {
        let mut salt = [0u8; SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        Ok(Self {
            algorithm: "pbkdf2-hmac-sha256".to_owned(),
            iterations: kdf_iterations(),
            salt: BASE64.encode(salt),
            version: CONTAINER_VERSION,
        })
    }

    /// JSON-представление (хранится в таблице `vault`).
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string(&serde_json::json!({
            "algorithm": self.algorithm,
            "iterations": self.iterations,
            "salt": self.salt,
            "version": self.version,
        }))?)
    }

    /// Разбор JSON; неизвестный алгоритм отвергается сразу.
    pub fn from_json(raw: &str) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_str(raw)?;
        let field = |name: &str| {
            Error::vault(
                format!("в параметрах сейфа нет поля {name}"),
                VaultErrorCode::BadFormat,
            )
        };
        let algorithm = value
            .get("algorithm")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| field("algorithm"))?;
        if algorithm != "pbkdf2-hmac-sha256" {
            return Err(Error::vault(
                format!("неподдерживаемый алгоритм вывода ключа: {algorithm}"),
                VaultErrorCode::BadFormat,
            ));
        }
        let iterations = value
            .get("iterations")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| field("iterations"))?;
        let salt = value
            .get("salt")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| field("salt"))?
            .to_owned();
        let version = value
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(u64::from(CONTAINER_VERSION)) as u8;
        if version > CONTAINER_VERSION {
            return Err(Error::vault(
                format!("формат сейфа версии {version} новее поддерживаемого"),
                VaultErrorCode::UnsupportedVersion,
            ));
        }
        Ok(Self {
            algorithm: algorithm.to_owned(),
            iterations: u32::try_from(iterations).unwrap_or_else(|_| kdf_iterations()),
            salt,
            version,
        })
    }

    /// Соль в виде байтов.
    pub fn salt_bytes(&self) -> Result<Vec<u8>> {
        BASE64
            .decode(self.salt.as_bytes())
            .map_err(|_| Error::vault("соль сейфа повреждена", VaultErrorCode::BadFormat))
    }
}

/// Запись сейфа из таблицы `vault`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultRecord {
    /// Параметры вывода ключа.
    pub params: KdfParams,
    /// Хеш ключа-верификатора (hex) — по нему проверяется пароль.
    pub verifier: String,
}

impl VaultRecord {
    /// Собирает запись из значений, прочитанных из базы.
    pub fn from_parts(params_json: String, verifier: String) -> Result<Self> {
        if verifier.is_empty() {
            return Err(Error::vault(
                "пустой верификатор сейфа",
                VaultErrorCode::BadFormat,
            ));
        }
        Ok(Self {
            params: KdfParams::from_json(&params_json)?,
            verifier,
        })
    }

    /// JSON параметров для записи в базу.
    pub fn params_json(&self) -> Result<String> {
        self.params.to_json()
    }
}

/// Кэш ключа разблокированного сейфа.
///
/// Держит ровно одну пару «верификатор → ключ»: повторная инициализация или
/// смена пароля заменяют её, блокировка — очищает. Ключ живёт только в памяти
/// процесса и обнуляется при удалении.
#[derive(Debug, Default)]
pub struct VaultSession {
    inner: Mutex<Option<(String, Vec<u8>)>>,
}

impl VaultSession {
    /// Запоминает ключ для конкретного верификатора.
    pub fn remember(&self, verifier: String, key: Vec<u8>) {
        if let Ok(mut guard) = self.inner.lock() {
            if let Some((_, previous)) = guard.as_mut() {
                zeroize(previous);
            }
            *guard = Some((verifier, key));
        }
    }

    /// Заменяет текущую пару (инициализация и смена пароля).
    pub fn replace(&self, verifier: String, key: Vec<u8>) {
        self.remember(verifier, key);
    }

    /// Ключ, если сейф разблокирован и верификатор совпадает.
    pub fn key_for(&self, verifier: &str) -> Option<Vec<u8>> {
        let guard = self.inner.lock().ok()?;
        guard
            .as_ref()
            .filter(|(stored, _)| stored == verifier)
            .map(|(_, key)| key.clone())
    }

    /// Разблокирован ли сейф.
    pub fn is_unlocked(&self) -> bool {
        self.inner
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    /// Заблокировать: ключ обнуляется и удаляется из памяти.
    pub fn clear(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            if let Some((_, key)) = guard.as_mut() {
                zeroize(key);
            }
            *guard = None;
        }
    }
}

/// Проверяет пароль по политике приложения.
pub fn validate_password(password: &str) -> Result<()> {
    let length = password.chars().count();
    if length < MIN_PASSWORD_CHARS {
        return Err(Error::invalid_field(
            format!("пароль короче {MIN_PASSWORD_CHARS} символов"),
            "password",
        ));
    }
    if length > MAX_PASSWORD_CHARS {
        return Err(Error::invalid_field(
            format!("пароль длиннее {MAX_PASSWORD_CHARS} символов"),
            "password",
        ));
    }
    if password.chars().all(|character| character.is_whitespace()) {
        return Err(Error::invalid_field(
            "пароль не может состоять из пробелов",
            "password",
        ));
    }
    Ok(())
}

/// Создаёт сейф: новые параметры, верификатор и ключ шифрования.
pub fn initialize(password: &str) -> Result<(VaultRecord, Vec<u8>)> {
    validate_password(password)?;
    let params = KdfParams::generate()?;
    build_record(params, password)
}

/// Создаёт запись сейфа с заданными параметрами.
///
/// Отдельная функция нужна тестам: штатные 210 000 итераций PBKDF2 сделали бы
/// прогон десятков тестов неприлично долгим, поэтому тесты подставляют
/// уменьшенное число итераций, проверяя ту же логику.
pub fn build_record(params: KdfParams, password: &str) -> Result<(VaultRecord, Vec<u8>)> {
    let mut master = derive_master(&params, password)?;
    let record = VaultRecord {
        verifier: verifier_hex(&master),
        params,
    };
    let key = encryption_key(&master);
    zeroize(&mut master);
    Ok((record, key))
}

/// Выводит мастер-ключ из пароля и параметров.
pub fn derive_master(params: &KdfParams, password: &str) -> Result<Vec<u8>> {
    let salt = params.salt_bytes()?;
    let mut master = vec![0u8; KEY_LEN];
    pbkdf2::<Hmac<Sha256>>(password.as_bytes(), &salt, params.iterations, &mut master)
        .map_err(|_| Error::vault("не удалось вывести ключ", VaultErrorCode::BadFormat))?;
    Ok(master)
}

/// Ключ шифрования из мастер-ключа.
pub fn encryption_key(master: &[u8]) -> Vec<u8> {
    blake3::derive_key("bookmarks vault encryption v1", master).to_vec()
}

/// Ключ верификатора из мастер-ключа.
pub fn verification_key(master: &[u8]) -> Vec<u8> {
    blake3::derive_key("bookmarks vault verifier v1", master).to_vec()
}

/// Хеш ключа-верификатора, который сохраняется в базе.
pub fn verifier_hex(master: &[u8]) -> String {
    blake3::hash(&verification_key(master)).to_hex().to_string()
}

/// Проверяет пароль и возвращает ключ шифрования.
pub fn unlock(record: &VaultRecord, password: &str) -> Result<Vec<u8>> {
    let mut master = derive_master(&record.params, password)?;
    let candidate = verifier_hex(&master);
    // Сравнение за время, не зависящее от позиции первого отличия.
    let matched = constant_time_eq(candidate.as_bytes(), record.verifier.as_bytes());
    let key = encryption_key(&master);
    zeroize(&mut master);
    if matched {
        Ok(key)
    } else {
        Err(Error::vault(
            "неверный пароль сейфа",
            VaultErrorCode::BadPassword,
        ))
    }
}

/// Шифрует данные в контейнер `BKENC1` (base64).
pub fn encrypt_container(key: &[u8], plaintext: &[u8]) -> Result<String> {
    let bytes = encrypt_container_bytes(key, plaintext)?;
    Ok(BASE64.encode(bytes))
}

/// Шифрует данные в байты контейнера (для вложений и файлов синхронизации).
pub fn encrypt_container_bytes(key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
    if key.len() != KEY_LEN {
        return Err(Error::vault(
            "ключ сейфа неверной длины",
            VaultErrorCode::BadFormat,
        ));
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| {
        Error::vault(
            "не удалось инициализировать шифр",
            VaultErrorCode::BadFormat,
        )
    })?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let mut container = Vec::with_capacity(header_len() + plaintext.len() + 16);
    container.extend_from_slice(MAGIC);
    container.push(CONTAINER_VERSION);
    // Соль контейнера отличается от соли KDF и заполняется случайно для
    // каждого шифрования: одинаковые заметки не должны давать одинаковый
    // шифртекст.
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    container.extend_from_slice(&salt);
    container.extend_from_slice(nonce.as_slice());

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| Error::vault("сбой шифрования", VaultErrorCode::BadFormat))?;
    container.extend_from_slice(&ciphertext);
    Ok(container)
}

/// Расшифровывает контейнер `BKENC1` из base64 в текст.
pub fn decrypt_container(key: &[u8], encoded: &str) -> Result<String> {
    let container = BASE64.decode(encoded.trim().as_bytes()).map_err(|_| {
        Error::vault(
            "контейнер не является корректным base64",
            VaultErrorCode::BadFormat,
        )
    })?;
    let plaintext = decrypt_container_bytes(key, &container)?;
    String::from_utf8(plaintext).map_err(|_| {
        Error::vault(
            "расшифрованные данные не являются текстом",
            VaultErrorCode::BadFormat,
        )
    })
}

/// Расшифровывает контейнер в байты (для вложений).
pub fn decrypt_container_bytes(key: &[u8], container: &[u8]) -> Result<Vec<u8>> {
    let header = header_len();
    if container.len() < header {
        return Err(Error::vault(
            "контейнер слишком короткий",
            VaultErrorCode::BadFormat,
        ));
    }
    if &container[..MAGIC.len()] != MAGIC {
        return Err(Error::vault(
            "файл не является зашифрованными данными Bookmarks",
            VaultErrorCode::BadFormat,
        ));
    }
    let version = container[MAGIC.len()];
    if version > CONTAINER_VERSION {
        return Err(Error::vault(
            format!("контейнер версии {version} новее поддерживаемого"),
            VaultErrorCode::UnsupportedVersion,
        ));
    }
    if key.len() != KEY_LEN {
        return Err(Error::vault(
            "ключ сейфа неверной длины",
            VaultErrorCode::BadFormat,
        ));
    }

    let nonce_start = MAGIC.len() + 1 + SALT_LEN;
    let nonce = Nonce::from_slice(&container[nonce_start..header]);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| {
        Error::vault(
            "не удалось инициализировать шифр",
            VaultErrorCode::BadFormat,
        )
    })?;
    cipher.decrypt(nonce, &container[header..]).map_err(|_| {
        Error::vault(
            "неверный пароль или повреждённые данные",
            VaultErrorCode::BadPassword,
        )
    })
}

/// Размер заголовка контейнера.
const fn header_len() -> usize {
    MAGIC.len() + 1 + SALT_LEN + NONCE_LEN
}

/// Сравнивает байты за время, не зависящее от позиции первого отличия.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        difference |= a ^ b;
    }
    difference == 0
}

/// Перезаписывает секрет нулями.
///
/// Полной гарантии удаления из памяти в Rust нет (компилятор вправе
/// оптимизировать запись), но это убирает очевидные следы ключа в куче и в
/// дампах процесса.
fn zeroize(secret: &mut [u8]) {
    for byte in secret.iter_mut() {
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Параметры с уменьшенным числом итераций — только для тестов.
    fn fast_params() -> KdfParams {
        set_kdf_iterations_for_tests(1_000);
        KdfParams::generate().expect("params")
    }

    fn fast_record(password: &str) -> (VaultRecord, Vec<u8>) {
        build_record(fast_params(), password).expect("record")
    }

    #[test]
    fn encrypts_and_decrypts_roundtrip() {
        let (_, key) = fast_record("пароль-достаточно-длинный");
        let container = encrypt_container(&key, "секретная заметка".as_bytes()).expect("encrypt");
        assert!(
            container.starts_with("QktFTkMx"),
            "base64 of the BKENC1 magic"
        );
        assert!(!container.contains("секрет"), "plaintext must not leak");
        assert_eq!(
            decrypt_container(&key, &container).expect("decrypt"),
            "секретная заметка"
        );
    }

    #[test]
    fn same_plaintext_gives_different_containers() {
        let (_, key) = fast_record("пароль-достаточно-длинный");
        let first = encrypt_container(&key, b"odnako").expect("encrypt");
        let second = encrypt_container(&key, b"odnako").expect("encrypt");
        assert_ne!(
            first, second,
            "random nonce must produce distinct ciphertext"
        );
    }

    #[test]
    fn wrong_key_is_rejected() {
        let (_, key) = fast_record("правильный-пароль-1");
        let (_, other) = fast_record("неправильный-пароль-2");
        let container = encrypt_container(&key, b"secret").expect("encrypt");
        let error = decrypt_container(&other, &container).expect_err("must fail");
        assert!(matches!(
            error,
            Error::Vault {
                code: VaultErrorCode::BadPassword,
                ..
            }
        ));
    }

    #[test]
    fn tampered_container_is_rejected() {
        let (_, key) = fast_record("правильный-пароль-1");
        let container = encrypt_container(&key, b"secret data").expect("encrypt");
        let mut bytes = BASE64.decode(container.as_bytes()).expect("decode");
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        assert!(decrypt_container(&key, &BASE64.encode(bytes)).is_err());
    }

    #[test]
    fn foreign_file_is_rejected_by_magic() {
        let (_, key) = fast_record("правильный-пароль-1");
        let foreign = BASE64.encode(b"KMPBENC1-not-ours-at-all-payload");
        let error = decrypt_container(&key, &foreign).expect_err("must fail");
        assert!(matches!(
            error,
            Error::Vault {
                code: VaultErrorCode::BadFormat,
                ..
            }
        ));
    }

    #[test]
    fn newer_version_is_reported_separately() {
        let (_, key) = fast_record("правильный-пароль-1");
        let mut bytes = BASE64
            .decode(encrypt_container(&key, b"x").expect("encrypt").as_bytes())
            .expect("decode");
        bytes[MAGIC.len()] = CONTAINER_VERSION + 1;
        let error = decrypt_container(&key, &BASE64.encode(bytes)).expect_err("must fail");
        assert!(matches!(
            error,
            Error::Vault {
                code: VaultErrorCode::UnsupportedVersion,
                ..
            }
        ));
    }

    #[test]
    fn truncated_container_is_rejected() {
        let (_, key) = fast_record("правильный-пароль-1");
        assert!(matches!(
            decrypt_container(&key, &BASE64.encode(b"BKENC1\x01too-short")),
            Err(Error::Vault {
                code: VaultErrorCode::BadFormat,
                ..
            })
        ));
    }

    #[test]
    fn verifier_matches_only_correct_password() {
        let (record, _) = fast_record("один-пароль-длинный");
        assert!(unlock(&record, "один-пароль-длинный").is_ok());
        let error = unlock(&record, "другой-пароль-длинный").expect_err("must fail");
        assert!(matches!(
            error,
            Error::Vault {
                code: VaultErrorCode::BadPassword,
                ..
            }
        ));
    }

    #[test]
    fn params_roundtrip_through_json() {
        let params = fast_params();
        let json = params.to_json().expect("json");
        assert_eq!(KdfParams::from_json(&json).expect("parse"), params);
    }

    #[test]
    fn params_reject_unknown_algorithm_and_new_version() {
        let broken = r#"{"algorithm":"argon3","iterations":1,"salt":"AAAA","version":1}"#;
        assert!(matches!(
            KdfParams::from_json(broken),
            Err(Error::Vault {
                code: VaultErrorCode::BadFormat,
                ..
            })
        ));
        let future =
            r#"{"algorithm":"pbkdf2-hmac-sha256","iterations":1,"salt":"AAAA","version":9}"#;
        assert!(matches!(
            KdfParams::from_json(future),
            Err(Error::Vault {
                code: VaultErrorCode::UnsupportedVersion,
                ..
            })
        ));
        assert!(KdfParams::from_json("не json").is_err());
    }

    #[test]
    fn record_rejects_empty_verifier() {
        let params = fast_params();
        let error = VaultRecord::from_parts(params.to_json().expect("json"), String::new())
            .expect_err("must fail");
        assert!(matches!(
            error,
            Error::Vault {
                code: VaultErrorCode::BadFormat,
                ..
            }
        ));
    }

    #[test]
    fn password_policy_is_enforced() {
        assert!(matches!(
            validate_password("корот"),
            Err(Error::InvalidInput { .. })
        ));
        assert!(validate_password(&"д".repeat(MAX_PASSWORD_CHARS + 1)).is_err());
        assert!(validate_password("        ").is_err());
        assert!(validate_password("нормальный-пароль").is_ok());
    }

    #[test]
    fn session_cache_follows_verifier() {
        let session = VaultSession::default();
        assert!(!session.is_unlocked());
        assert!(session.key_for("abc").is_none());

        session.remember("abc".into(), vec![1, 2, 3]);
        assert!(session.is_unlocked());
        assert_eq!(session.key_for("abc").expect("key"), vec![1, 2, 3]);
        // Чужой верификатор (смена пароля) не отдаёт старый ключ.
        assert!(session.key_for("xyz").is_none());

        session.replace("xyz".into(), vec![9]);
        assert_eq!(session.key_for("xyz").expect("key"), vec![9]);
        assert!(session.key_for("abc").is_none());

        session.clear();
        assert!(!session.is_unlocked());
    }

    #[test]
    fn key_domain_separation_holds() {
        let master = vec![7u8; KEY_LEN];
        assert_ne!(encryption_key(&master), verification_key(&master));
        assert_eq!(encryption_key(&master), encryption_key(&master));
    }

    #[test]
    fn handles_empty_and_large_payloads() {
        let (_, key) = fast_record("правильный-пароль-1");
        let empty = encrypt_container(&key, b"").expect("encrypt");
        assert_eq!(decrypt_container(&key, &empty).expect("decrypt"), "");

        let large = vec![b'x'; 2 * 1024 * 1024];
        let container = encrypt_container_bytes(&key, &large).expect("encrypt");
        assert_eq!(
            decrypt_container_bytes(&key, &container).expect("decrypt"),
            large
        );
    }

    #[test]
    fn rejects_key_of_wrong_length() {
        assert!(encrypt_container(&[0u8; KEY_LEN], b"x").is_ok());
        assert!(matches!(
            encrypt_container(&[0u8; 16], b"x"),
            Err(Error::Vault { .. })
        ));
        assert!(matches!(
            decrypt_container_bytes(&[0u8; 8], b"BKENC1\x01aaaaaaaaaaaaaaaabbbbbbbbbbbbcc"),
            Err(Error::Vault { .. })
        ));
    }
}
