//! Настройки приложения.
//!
//! Настройки живут в отдельном JSON-файле (`settings.json`), а не в базе
//! заметок: они должны читаться до открытия хранилища и оставаться доступными,
//! даже если база повреждена или зашифрована. Структура версионируется —
//! неизвестные поля игнорируются, отсутствующие получают значения по
//! умолчанию, поэтому файл от старой версии приложения открывается новой.

use serde::{Deserialize, Serialize};

use crate::ids::EntityId;

/// Текущая версия формата настроек.
pub const SETTINGS_VERSION: u32 = 1;

/// Тема оформления.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Светлая.
    Light,
    /// Тёмная.
    Dark,
    /// Как в системе.
    #[default]
    System,
}

/// Акцентный цвет интерфейса.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AccentColor {
    /// Синий (по умолчанию).
    #[default]
    Blue,
    /// Голубой.
    Cyan,
    /// Зелёный.
    Green,
    /// Янтарный.
    Amber,
    /// Красный.
    Red,
    /// Розовый.
    Pink,
    /// Фиолетовый.
    Violet,
    /// Графитовый (нейтральный интерфейс без акцента).
    Slate,
}

impl AccentColor {
    /// CSS-переменные акцента: базовый цвет, ховер и градиент для активных состояний.
    ///
    /// Значения подобраны парами «светлая/тёмная тема» так, чтобы контраст
    /// текста на акценте оставался не ниже WCAG AA.
    pub fn css_variables(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Blue => (
                "#2563eb",
                "#1d4ed8",
                "linear-gradient(180deg,#3b82f6 0%,#2563eb 45%,#1d4ed8 100%)",
            ),
            Self::Cyan => (
                "#0891b2",
                "#0e7490",
                "linear-gradient(180deg,#22d3ee 0%,#0891b2 45%,#0e7490 100%)",
            ),
            Self::Green => (
                "#059669",
                "#047857",
                "linear-gradient(180deg,#34d399 0%,#059669 45%,#047857 100%)",
            ),
            Self::Amber => (
                "#d97706",
                "#b45309",
                "linear-gradient(180deg,#fbbf24 0%,#d97706 45%,#b45309 100%)",
            ),
            Self::Red => (
                "#dc2626",
                "#b91c1c",
                "linear-gradient(180deg,#f87171 0%,#dc2626 45%,#b91c1c 100%)",
            ),
            Self::Pink => (
                "#db2777",
                "#be185d",
                "linear-gradient(180deg,#f472b6 0%,#db2777 45%,#be185d 100%)",
            ),
            Self::Violet => (
                "#7c3aed",
                "#6d28d9",
                "linear-gradient(180deg,#a78bfa 0%,#7c3aed 45%,#6d28d9 100%)",
            ),
            Self::Slate => (
                "#475569",
                "#334155",
                "linear-gradient(180deg,#64748b 0%,#475569 45%,#334155 100%)",
            ),
        }
    }
}

/// Язык интерфейса.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// Русский.
    #[default]
    Ru,
    /// Английский.
    En,
}

impl Language {
    /// Код локали для i18next.
    pub fn as_locale(self) -> &'static str {
        match self {
            Self::Ru => "ru",
            Self::En => "en",
        }
    }
}

/// Порядок сортировки списка заметок.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    /// Сначала изменённые позже.
    #[default]
    UpdatedDesc,
    /// Сначала изменённые раньше.
    UpdatedAsc,
    /// Сначала созданные позже.
    CreatedDesc,
    /// Сначала созданные раньше.
    CreatedAsc,
    /// По заголовку (А→Я).
    TitleAsc,
    /// По заголовку (Я→А).
    TitleDesc,
}

/// Настройки редактора.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EditorSettings {
    /// Размер шрифта редактора, px.
    pub font_size: u8,
    /// Моноширинный шрифт вместо пропорционального.
    pub monospace: bool,
    /// Ширина строки ограничена (как в книжной вёрстке) или на всю ширину окна.
    pub comfortable_width: bool,
    /// Перенос длинных строк.
    pub word_wrap: bool,
    /// Показывать номер строки.
    pub line_numbers: bool,
    /// Проверка орфографии (поддержка WebView).
    pub spellcheck: bool,
    /// Автосохранение при паузе ввода.
    pub autosave: bool,
    /// Задержка автосохранения, мс.
    pub autosave_delay_ms: u32,
    /// Режим по умолчанию: только редактор, только просмотр или две панели.
    pub default_view: EditorView,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font_size: 15,
            monospace: false,
            comfortable_width: true,
            word_wrap: true,
            line_numbers: false,
            spellcheck: true,
            autosave: true,
            autosave_delay_ms: 600,
            default_view: EditorView::Split,
        }
    }
}

/// Режим отображения редактора.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EditorView {
    /// Только исходный Markdown.
    Edit,
    /// Только отрендеренный вид.
    Preview,
    /// Две панели рядом.
    #[default]
    Split,
}

/// Настройки интерфейса.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InterfaceSettings {
    /// Тема.
    pub theme: Theme,
    /// Акцентный цвет.
    pub accent: AccentColor,
    /// Язык.
    pub language: Language,
    /// Анимации перехода между экранами и списками.
    pub animations: bool,
    /// Плотность списка: комфортная или компактная.
    pub compact_list: bool,
    /// Показывать панель папок.
    pub show_sidebar: bool,
    /// Показывать превью заметки в списке.
    pub show_excerpts: bool,
    /// Порядок сортировки списка.
    pub sort_order: SortOrder,
    /// Ширина панели папок, px (сохраняется между запусками).
    pub sidebar_width: u16,
}

impl Default for InterfaceSettings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            accent: AccentColor::Blue,
            language: Language::Ru,
            animations: true,
            compact_list: false,
            show_sidebar: true,
            show_excerpts: true,
            sort_order: SortOrder::UpdatedDesc,
            sidebar_width: 300,
        }
    }
}

/// Политика хранения версий заметки.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VersionPolicy {
    /// Не хранить версии.
    Off,
    /// Хранить последние N версий каждой заметки.
    #[default]
    KeepRecent,
    /// Хранить все версии, но удалять те, что старше N дней.
    KeepDays,
}

/// Настройки истории версий.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VersionSettings {
    /// Политика хранения.
    pub policy: VersionPolicy,
    /// Число версий на заметку (для `keep_recent`).
    pub keep_count: u16,
    /// Возраст версий в днях (для `keep_days`).
    pub keep_days: u16,
}

impl Default for VersionSettings {
    fn default() -> Self {
        Self {
            policy: VersionPolicy::KeepRecent,
            keep_count: 50,
            keep_days: 90,
        }
    }
}

/// Тип удалённого хранилища синхронизации.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RemoteKind {
    /// Синхронизация выключена.
    #[default]
    None,
    /// Локальная или сетевая папка (SMB, внешний диск).
    Directory,
    /// WebDAV (Nextcloud, Яндекс.Диск, davmail и т. п.).
    WebDav,
}

/// Настройки синхронизации.
///
/// Секреты (пароль WebDAV) здесь не хранятся: они попадают в системное
/// хранилище учётных данных (Windows Credential Manager / keychain), а в
/// настройках остаётся только факт их наличия.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SyncSettings {
    /// Тип хранилища.
    pub remote: RemoteKind,
    /// Адрес: путь к папке или URL WebDAV.
    pub location: String,
    /// Логин WebDAV (не секрет).
    pub username: String,
    /// Автоматическая синхронизация по интервалу.
    pub auto_sync: bool,
    /// Интервал автосинхронизации, секунды.
    pub interval_secs: u32,
    /// Синхронизировать при запуске приложения.
    pub sync_on_start: bool,
    /// Хранить в облаке только зашифрованные данные (E2EE).
    pub encrypt_remote: bool,
    /// Идентификатор устройства — участвует в разрешении конфликтов.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<EntityId>,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            remote: RemoteKind::None,
            location: String::new(),
            username: String::new(),
            auto_sync: true,
            interval_secs: 300,
            sync_on_start: true,
            encrypt_remote: true,
            device_id: None,
        }
    }
}

/// Настройки сейфа (шифрования части заметок).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VaultSettings {
    /// Сейф создан.
    pub enabled: bool,
    /// Автоматически блокировать сейф после бездействия.
    pub auto_lock: bool,
    /// Время бездействия до блокировки, минуты.
    pub auto_lock_minutes: u16,
    /// Блокировать сейф при сворачивании окна.
    pub lock_on_minimize: bool,
}

impl Default for VaultSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_lock: true,
            auto_lock_minutes: 10,
            lock_on_minimize: false,
        }
    }
}

/// Настройки резервных копий.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BackupSettings {
    /// Делать автоматические копии.
    pub enabled: bool,
    /// Интервал, часы.
    pub interval_hours: u16,
    /// Сколько копий хранить.
    pub keep_count: u16,
    /// Папка для копий (пусто — стандартная в каталоге приложения).
    pub directory: String,
}

impl Default for BackupSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_hours: 24,
            keep_count: 7,
            directory: String::new(),
        }
    }
}

/// Корневой объект настроек.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Версия формата.
    pub version: u32,
    /// Интерфейс.
    pub interface: InterfaceSettings,
    /// Редактор.
    pub editor: EditorSettings,
    /// История версий.
    pub versions: VersionSettings,
    /// Синхронизация.
    pub sync: SyncSettings,
    /// Сейф.
    pub vault: VaultSettings,
    /// Резервные копии.
    pub backup: BackupSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            interface: InterfaceSettings::default(),
            editor: EditorSettings::default(),
            versions: VersionSettings::default(),
            sync: SyncSettings::default(),
            vault: VaultSettings::default(),
            backup: BackupSettings::default(),
        }
    }
}

impl Settings {
    /// Разбирает JSON настроек, подставляя значения по умолчанию для новых полей.
    ///
    /// Файл от более новой версии принимается, но `version` приводится к
    /// текущей: неизвестные настройки теряются осознанно, а не ломают запуск.
    pub fn from_json(raw: &str) -> crate::error::Result<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(Self::default());
        }
        let mut settings: Self = serde_json::from_str(trimmed)?;
        settings.version = SETTINGS_VERSION;
        Ok(settings)
    }

    /// Сериализует настройки в форматированный JSON (удобно читать и диффить).
    pub fn to_json(&self) -> crate::error::Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Ограничивает числовые значения допустимыми диапазонами.
    ///
    /// Вызывается после загрузки из файла: пользователь мог отредактировать
    /// JSON вручную и поставить `fontSize: 0`, что сломало бы вёрстку.
    pub fn clamp(&mut self) {
        self.editor.font_size = self.editor.font_size.clamp(11, 28);
        self.editor.autosave_delay_ms = self.editor.autosave_delay_ms.clamp(200, 10_000);
        self.interface.sidebar_width = self.interface.sidebar_width.clamp(200, 560);
        self.versions.keep_count = self.versions.keep_count.clamp(1, 500);
        self.versions.keep_days = self.versions.keep_days.clamp(1, 3650);
        self.sync.interval_secs = self.sync.interval_secs.clamp(30, 86_400);
        self.vault.auto_lock_minutes = self.vault.auto_lock_minutes.clamp(1, 480);
        self.backup.interval_hours = self.backup.interval_hours.clamp(1, 168);
        self.backup.keep_count = self.backup.keep_count.clamp(1, 100);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_roundtrip_through_json() {
        let settings = Settings::default();
        let json = settings.to_json().expect("serializable");
        let parsed = Settings::from_json(&json).expect("parsable");
        assert_eq!(parsed, settings);
        assert_eq!(parsed.version, SETTINGS_VERSION);
    }

    #[test]
    fn empty_file_yields_defaults() {
        assert_eq!(
            Settings::from_json("").expect("defaults"),
            Settings::default()
        );
        assert_eq!(
            Settings::from_json("   \n ").expect("defaults"),
            Settings::default()
        );
    }

    #[test]
    fn unknown_fields_are_ignored_and_missing_filled() {
        let json = r#"{ "version": 99, "interface": { "theme": "dark", "futureFlag": true } }"#;
        let settings = Settings::from_json(json).expect("parsable");
        assert_eq!(settings.interface.theme, Theme::Dark);
        assert_eq!(settings.version, SETTINGS_VERSION);
        // Поле из будущего не сломало остальные значения по умолчанию.
        assert_eq!(settings.editor.font_size, 15);
        assert_eq!(settings.sync.remote, RemoteKind::None);
    }

    #[test]
    fn clamps_absurd_values() {
        let mut settings = Settings::default();
        settings.editor.font_size = 0;
        settings.editor.autosave_delay_ms = 1;
        settings.interface.sidebar_width = 10_000;
        settings.sync.interval_secs = 5;
        settings.versions.keep_count = 0;
        settings.clamp();
        assert_eq!(settings.editor.font_size, 11);
        assert_eq!(settings.editor.autosave_delay_ms, 200);
        assert_eq!(settings.interface.sidebar_width, 560);
        assert_eq!(settings.sync.interval_secs, 30);
        assert_eq!(settings.versions.keep_count, 1);
    }

    #[test]
    fn accent_palette_has_contrast_pairs() {
        for accent in [
            AccentColor::Blue,
            AccentColor::Cyan,
            AccentColor::Green,
            AccentColor::Amber,
            AccentColor::Red,
            AccentColor::Pink,
            AccentColor::Violet,
            AccentColor::Slate,
        ] {
            let (base, hover, gradient) = accent.css_variables();
            assert!(base.starts_with('#') && base.len() == 7);
            assert!(hover.starts_with('#') && hover.len() == 7);
            assert!(gradient.starts_with("linear-gradient"));
            assert_ne!(base, hover);
        }
    }

    #[test]
    fn language_codes_match_locale_files() {
        assert_eq!(Language::Ru.as_locale(), "ru");
        assert_eq!(Language::En.as_locale(), "en");
    }

    #[test]
    fn rejects_broken_json_with_error() {
        assert!(Settings::from_json("{ not json").is_err());
    }
}
