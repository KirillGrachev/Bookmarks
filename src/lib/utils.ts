import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

/** Объединяет классы Tailwind с разрешением конфликтов (`p-2` + `p-4` → `p-4`). */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}

/** Форматирует число байт в человекочитаемый размер. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return '0 Б';
  }
  const units = ['Б', 'КБ', 'МБ', 'ГБ', 'ТБ'];
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** exponent;
  const digits = value >= 100 || exponent === 0 ? 0 : 1;
  return `${value.toFixed(digits)} ${units[exponent]}`;
}

/** Форматирует количество как «N заметок/заметка/заметки» по правилам русского. */
export function pluralRu(count: number, one: string, few: string, many: string): string {
  const mod10 = count % 10;
  const mod100 = count % 100;
  if (mod10 === 1 && mod100 !== 11) {
    return `${count} ${one}`;
  }
  if (mod10 >= 2 && mod10 <= 4 && (mod100 < 10 || mod100 >= 20)) {
    return `${count} ${few}`;
  }
  return `${count} ${many}`;
}

/** Обрезает строку по символам (не по байтам), добавляя «…» при необходимости. */
export function truncate(value: string, maxChars: number): string {
  const chars = [...value];
  if (chars.length <= maxChars) {
    return value;
  }
  return `${chars.slice(0, Math.max(1, maxChars - 1)).join('')}…`;
}

/** Защищённая задержка: таймер можно отменить без проверки на null. */
export function createDebouncer() {
  let timer: ReturnType<typeof setTimeout> | undefined;
  return {
    run(delayMs: number, action: () => void): void {
      if (timer) {
        clearTimeout(timer);
      }
      timer = setTimeout(action, delayMs);
    },
    cancel(): void {
      if (timer) {
        clearTimeout(timer);
        timer = undefined;
      }
    },
    flush(): void {
      if (timer) {
        clearTimeout(timer);
        timer = undefined;
      }
    },
  };
}
