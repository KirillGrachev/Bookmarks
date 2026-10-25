import { describe, expect, it } from 'vitest';

import { dict, resolveLanguage, translate, type DictKey } from '../i18n';
import { formatBytes, pluralRu, truncate } from '../utils';
import { clampSettings, defaultSettings, withDefaults } from '../settingsDefaults';
import { formatRelative, startOfLocalDay } from '../time';

describe('словарь переводов', () => {
  it('английский словарь покрывает все ключи русского', () => {
    const ruKeys = Object.keys(dict.ru) as DictKey[];
    const enKeys = new Set(Object.keys(dict.en));
    const missing = ruKeys.filter((key) => !enKeys.has(key));
    expect(missing).toEqual([]);
  });

  it('в английском нет лишних ключей', () => {
    const ruKeys = new Set(Object.keys(dict.ru));
    const extra = Object.keys(dict.en).filter((key) => !ruKeys.has(key));
    expect(extra).toEqual([]);
  });

  it('незаполненных значений нет', () => {
    for (const [lang, table] of Object.entries(dict)) {
      for (const [key, value] of Object.entries(table)) {
        expect(value, `${lang}:${key}`).toBeTruthy();
        expect(typeof value, `${lang}:${key}`).toBe('string');
      }
    }
  });

  it('подставляет переменные', () => {
    expect(translate('ru', 'list.count', { count: 5 })).toBe('5 заметок');
    expect(translate('en', 'editor.words', { count: 12 })).toBe('12 words');
  });

  it('неизвестная переменная остаётся как есть', () => {
    expect(translate('ru', 'list.count')).toBe('{count} заметок');
  });

  it('неизвестный ключ возвращается ключом, а не пустой строкой', () => {
    expect(translate('ru', 'такого.ключа.нет' as DictKey)).toBe('такого.ключа.нет');
  });

  it('определяет язык по локали системы', () => {
    expect(resolveLanguage(null, ['ru-RU'])).toBe('ru');
    expect(resolveLanguage(null, ['en-US'])).toBe('en');
    expect(resolveLanguage(null, [])).toBe('en');
    // Явная настройка важнее системной локали.
    expect(resolveLanguage('en', ['ru-RU'])).toBe('en');
  });
});

describe('настройки', () => {
  it('значения по умолчанию стабильны', () => {
    const settings = defaultSettings();
    expect(settings.interface.theme).toBe('system');
    expect(settings.interface.language).toBe('ru');
    expect(settings.editor.autosave).toBe(true);
    expect(settings.sync.remote).toBe('none');
    expect(settings.vault.enabled).toBe(false);
  });

  it('частичный объект дополняется значениями по умолчанию', () => {
    const merged = withDefaults({ interface: { theme: 'dark' } as never });
    expect(merged.interface.theme).toBe('dark');
    expect(merged.interface.sidebarWidth).toBe(300);
    expect(merged.editor.fontSize).toBe(15);
  });

  it('пустые значения дают дефолты', () => {
    expect(withDefaults(null)).toEqual(defaultSettings());
    expect(withDefaults(undefined)).toEqual(defaultSettings());
  });

  it('абсурдные значения приводятся к диапазонам', () => {
    const settings = defaultSettings();
    settings.editor.fontSize = 0;
    settings.editor.autosaveDelayMs = 1;
    settings.interface.sidebarWidth = 10_000;
    settings.sync.intervalSecs = 5;
    settings.versions.keepCount = 0;
    const clamped = clampSettings(settings);
    expect(clamped.editor.fontSize).toBe(11);
    expect(clamped.editor.autosaveDelayMs).toBe(200);
    expect(clamped.interface.sidebarWidth).toBe(560);
    expect(clamped.sync.intervalSecs).toBe(30);
    expect(clamped.versions.keepCount).toBe(1);
  });
});

describe('утилиты', () => {
  it('formatBytes выбирает единицы', () => {
    expect(formatBytes(0)).toBe('0 Б');
    expect(formatBytes(512)).toBe('512 Б');
    expect(formatBytes(2048)).toBe('2.0 КБ');
    expect(formatBytes(5 * 1024 * 1024)).toBe('5.0 МБ');
    expect(formatBytes(Number.NaN)).toBe('0 Б');
  });

  it('pluralRu согласует падеж', () => {
    expect(pluralRu(1, 'заметка', 'заметки', 'заметок')).toBe('1 заметка');
    expect(pluralRu(2, 'заметка', 'заметки', 'заметок')).toBe('2 заметки');
    expect(pluralRu(5, 'заметка', 'заметки', 'заметок')).toBe('5 заметок');
    expect(pluralRu(11, 'заметка', 'заметки', 'заметок')).toBe('11 заметок');
    expect(pluralRu(21, 'заметка', 'заметки', 'заметок')).toBe('21 заметка');
  });

  it('truncate режет по символам, а не по байтам', () => {
    expect(truncate('привет', 10)).toBe('привет');
    expect(truncate('привет', 3)).toBe('пр…');
    expect(truncate('🚀🚀🚀', 2)).toBe('🚀…');
  });

  it('startOfLocalDay обнуляет время', () => {
    const value = new Date(2026, 4, 10, 15, 30, 45).getTime();
    const start = startOfLocalDay(value);
    const date = new Date(start);
    expect(date.getHours()).toBe(0);
    expect(date.getMinutes()).toBe(0);
    expect(date.getSeconds()).toBe(0);
  });

  it('formatRelative не падает на будущих и старых метках', () => {
    const now = Date.now();
    expect(typeof formatRelative(now, 'ru', now)).toBe('string');
    expect(typeof formatRelative(now + 3_600_000, 'ru', now)).toBe('string');
    expect(typeof formatRelative(now - 400 * 86_400_000, 'ru', now)).toBe('string');
  });
});
