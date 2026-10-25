import type { Settings } from '@/api/contracts';

/**
 * Значения настроек по умолчанию.
 *
 * Дублируют `Settings::default()` из ядра: в браузере (демо-режим) и до первого
 * ответа IPC интерфейс должен рисоваться сразу, а не ждать ответа. Расхождение
 * с Rust не критично — после загрузки настоящих настроек стор перезаписывается.
 */
export const SETTINGS_VERSION = 1;

export function defaultSettings(): Settings {
  return {
    version: SETTINGS_VERSION,
    interface: {
      theme: 'system',
      accent: 'blue',
      language: 'ru',
      animations: true,
      compactList: false,
      showSidebar: true,
      showExcerpts: true,
      sortOrder: 'updated_desc',
      sidebarWidth: 300,
    },
    editor: {
      fontSize: 15,
      monospace: false,
      comfortableWidth: true,
      wordWrap: true,
      lineNumbers: false,
      spellcheck: true,
      autosave: true,
      autosaveDelayMs: 600,
      defaultView: 'split',
    },
    versions: {
      policy: 'keep_recent',
      keepCount: 50,
      keepDays: 90,
    },
    sync: {
      remote: 'none',
      location: '',
      username: '',
      autoSync: true,
      intervalSecs: 300,
      syncOnStart: true,
      encryptRemote: true,
      deviceId: null,
    },
    vault: {
      enabled: false,
      autoLock: true,
      autoLockMinutes: 10,
      lockOnMinimize: false,
    },
    backup: {
      enabled: true,
      intervalHours: 24,
      keepCount: 7,
      directory: '',
    },
  };
}

/**
 * Дополняет загруженные настройки значениями по умолчанию.
 *
 * Файл настроек мог быть создан предыдущей версией приложения: неизвестные поля
 * игнорируются, отсутствующие — подставляются, поэтому интерфейс не получает
 * `undefined` там, где ожидает число или строку.
 */
export function withDefaults(settings: Partial<Settings> | null | undefined): Settings {
  const base = defaultSettings();
  if (!settings) {
    return base;
  }
  return {
    version: SETTINGS_VERSION,
    interface: { ...base.interface, ...settings.interface },
    editor: { ...base.editor, ...settings.editor },
    versions: { ...base.versions, ...settings.versions },
    sync: { ...base.sync, ...settings.sync },
    vault: { ...base.vault, ...settings.vault },
    backup: { ...base.backup, ...settings.backup },
  };
}

/** Ограничивает числовые настройки допустимыми диапазонами (как `clamp` в ядре). */
export function clampSettings(settings: Settings): Settings {
  const clamp = (value: number, min: number, max: number) => Math.min(Math.max(value, min), max);
  return {
    ...settings,
    editor: {
      ...settings.editor,
      fontSize: clamp(settings.editor.fontSize, 11, 28),
      autosaveDelayMs: clamp(settings.editor.autosaveDelayMs, 200, 10_000),
    },
    interface: {
      ...settings.interface,
      sidebarWidth: clamp(settings.interface.sidebarWidth, 200, 560),
    },
    versions: {
      ...settings.versions,
      keepCount: clamp(settings.versions.keepCount, 1, 500),
      keepDays: clamp(settings.versions.keepDays, 1, 3650),
    },
    sync: {
      ...settings.sync,
      intervalSecs: clamp(settings.sync.intervalSecs, 30, 86_400),
    },
    vault: {
      ...settings.vault,
      autoLockMinutes: clamp(settings.vault.autoLockMinutes, 1, 480),
    },
    backup: {
      ...settings.backup,
      intervalHours: clamp(settings.backup.intervalHours, 1, 168),
      keepCount: clamp(settings.backup.keepCount, 1, 100),
    },
  };
}
