/**
 * Форматирование времени.
 *
 * Метки приходят из ядра в миллисекундах UTC, а показывает их интерфейс в
 * локали пользователя — поэтому все функции принимают `locale` и не хранят
 * глобального состояния (иначе смена языка в настройках не меняла бы даты).
 */

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** «только что», «5 мин», «вчера», «12 мар» или «12 мар 2025». */
export function formatRelative(timestamp: number, locale: string, now = Date.now()): string {
  const diff = now - timestamp;
  if (diff < MINUTE && diff > -MINUTE) {
    return new Intl.RelativeTimeFormat(locale, { numeric: 'auto' }).format(0, 'minute');
  }
  if (Math.abs(diff) < HOUR) {
    const minutes = Math.round(diff / MINUTE);
    return new Intl.RelativeTimeFormat(locale, { numeric: 'auto' }).format(-minutes, 'minute');
  }
  if (Math.abs(diff) < DAY) {
    const hours = Math.round(diff / HOUR);
    return new Intl.RelativeTimeFormat(locale, { numeric: 'auto' }).format(-hours, 'hour');
  }
  if (Math.abs(diff) < 7 * DAY) {
    const days = Math.round(diff / DAY);
    return new Intl.RelativeTimeFormat(locale, { numeric: 'auto' }).format(-days, 'day');
  }
  const sameYear = new Date(timestamp).getFullYear() === new Date(now).getFullYear();
  return new Intl.DateTimeFormat(locale, {
    day: 'numeric',
    month: 'short',
    ...(sameYear ? {} : { year: 'numeric' }),
  }).format(timestamp);
}

/** Полная дата и время — для подсказок и экрана истории версий. */
export function formatFull(timestamp: number, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    day: 'numeric',
    month: 'long',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(timestamp);
}

/** Только дата — для группировки в списке версий. */
export function formatDate(timestamp: number, locale: string): string {
  return new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'long', year: 'numeric' }).format(
    timestamp,
  );
}

/** Только время — для подписи версии внутри дня. */
export function formatTime(timestamp: number, locale: string): string {
  return new Intl.DateTimeFormat(locale, { hour: '2-digit', minute: '2-digit' }).format(timestamp);
}

/** Длительность в человекочитаемом виде («1 ч 5 мин»). */
export function formatDuration(millis: number, locale: string): string {
  const seconds = Math.max(0, Math.round(millis / 1000));
  if (seconds < 60) {
    return new Intl.RelativeTimeFormat(locale, { numeric: 'always' })
      .format(-seconds, 'second')
      .replace(/^через\s+/i, '');
  }
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  if (minutes < 60) {
    return rest === 0 ? `${minutes} мин` : `${minutes} мин ${rest} с`;
  }
  const hours = Math.floor(minutes / 60);
  const restMinutes = minutes % 60;
  return restMinutes === 0 ? `${hours} ч` : `${hours} ч ${restMinutes} мин`;
}

/**
 * Граница «сегодня» в локальной зоне пользователя.
 *
 * Используется фильтром «изменённые сегодня»: сравнивать с UTC-полуночью
 * нельзя, иначе у пользователя в +03:00 утренние заметки попадали бы во вчера.
 */
export function startOfLocalDay(timestamp: number): number {
  const date = new Date(timestamp);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
}
