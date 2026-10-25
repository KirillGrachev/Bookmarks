import { isTauri } from '@/api/backend';

/**
 * Режим запуска приложения.
 *
 * `desktop` — окно Tauri, данные в SQLite, доступны сейф и синхронизация.
 * `browser-demo` — `npm run dev` без Rust: те же экраны на демо-хранилище,
 * чтобы вёрстку можно было править без тулчейна.
 */
export const runtimeMode: 'desktop' | 'browser-demo' = isTauri() ? 'desktop' : 'browser-demo';

/** Десктопный режим. */
export const isDesktop = runtimeMode === 'desktop';
