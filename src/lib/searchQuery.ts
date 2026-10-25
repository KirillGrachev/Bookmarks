/**
 * Разбор поискового запроса на фронтенде.
 *
 * Зеркалит `bookmarks-core::search::query`: тот же синтаксис, те же операторы.
 * Дублирование намеренное — оно нужно для мгновенной фильтрации уже загруженного
 * списка (без round-trip в Rust на каждое нажатие) и для подсветки операторов в
 * строке поиска. Расхождение поведения ловят тесты с обеих сторон.
 */

/** Текстовый токен запроса. */
export type QueryToken =
  { kind: 'prefix'; word: string; negated: boolean } | { kind: 'phrase'; phrase: string; negated: boolean };

/** Оператор-фильтр. */
export type FilterSpec =
  | { kind: 'tag'; name: string; negated: boolean }
  | { kind: 'folder'; name: string; negated: boolean }
  | { kind: 'flag'; flag: string; negated: boolean }
  | { kind: 'has'; type: string; negated: boolean }
  | { kind: 'color'; name: string; negated: boolean }
  | { kind: 'after'; millis: number }
  | { kind: 'before'; millis: number };

export interface ParsedQuery {
  text: QueryToken[];
  filters: FilterSpec[];
  /** Исходная строка — для отображения. */
  raw: string;
}

const KNOWN_OPERATORS = ['tag', 'folder', 'is', 'has', 'color', 'after', 'before'] as const;
const KNOWN_FLAGS = ['pinned', 'archived', 'encrypted', 'empty', 'attachment', 'untagged'] as const;
const KNOWN_HAS = ['image', 'link', 'code', 'task', 'table', 'attachment'] as const;

/** Нормализует метку: убирает лишние пробелы и переводы строк. */
function normalizeLabel(value: string): string {
  return value.replace(/\s+/g, ' ').trim();
}

/** Разбирает строку запроса. Неизвестные операторы считаются обычными словами. */
export function parseQuery(raw: string): ParsedQuery {
  const text: QueryToken[] = [];
  const filters: FilterSpec[] = [];
  const pattern = /"([^"]*)"|(\S+)/g;
  let match: RegExpExecArray | null;

  while ((match = pattern.exec(raw)) !== null) {
    if (match[1] !== undefined) {
      const phrase = match[1].trim();
      if (phrase) {
        text.push({ kind: 'phrase', phrase, negated: false });
      }
      continue;
    }
    classify(match[2], text, filters);
  }

  return { text, filters, raw };
}

function classify(token: string, text: QueryToken[], filters: FilterSpec[]): void {
  if (!token) {
    return;
  }
  const negated = token.startsWith('-') && token.length > 1;
  const body = negated ? token.slice(1) : token;

  // `#тег` — короткая форма фильтра.
  if (body.startsWith('#') && body.length > 1 && !/\s/.test(body)) {
    filters.push({ kind: 'tag', name: normalizeLabel(body.slice(1)).toLowerCase(), negated });
    return;
  }

  const colon = body.indexOf(':');
  if (colon > 0) {
    const operator = body.slice(0, colon).toLowerCase();
    const value = body.slice(colon + 1).trim();
    if (value && (KNOWN_OPERATORS as readonly string[]).includes(operator)) {
      const filter = buildFilter(operator, value, negated);
      if (filter) {
        filters.push(filter);
        return;
      }
    }
  }

  const cleaned = body.replace(/[*^(){}:"]/g, '').replace(/^-+|-+$/g, '');
  if (cleaned) {
    text.push({ kind: 'prefix', word: cleaned, negated });
  }
}

function buildFilter(operator: string, value: string, negated: boolean): FilterSpec | null {
  const normalized = normalizeLabel(value);
  switch (operator) {
    case 'tag':
      return { kind: 'tag', name: normalized.replace(/^#+/, '').toLowerCase(), negated };
    case 'folder':
      return { kind: 'folder', name: normalized.toLowerCase(), negated };
    case 'color':
      return { kind: 'color', name: normalized.toLowerCase(), negated };
    case 'is': {
      const flag = normalized.toLowerCase();
      return (KNOWN_FLAGS as readonly string[]).includes(flag) ? { kind: 'flag', flag, negated } : null;
    }
    case 'has': {
      const type = normalized.toLowerCase();
      return (KNOWN_HAS as readonly string[]).includes(type) ? { kind: 'has', type, negated } : null;
    }
    case 'after':
    case 'before': {
      const millis = parseDate(normalized, operator === 'before');
      if (millis === null) {
        return null;
      }
      return operator === 'after' ? { kind: 'after', millis } : { kind: 'before', millis };
    }
    default:
      return null;
  }
}

/** Разбирает дату: ISO, `дд.мм.гггг` или относительную (`today`, `7d`, `сегодня`). */
export function parseDate(value: string, endOfDay: boolean): number | null {
  const lowered = value.trim().toLowerCase();
  const today = new Date();
  today.setHours(0, 0, 0, 0);

  const offsetDays = (days: number): Date => {
    const date = new Date(today);
    date.setDate(date.getDate() - days);
    return date;
  };

  let date: Date | null = null;
  switch (lowered) {
    case 'today':
    case 'сегодня':
    case '0d':
      date = today;
      break;
    case 'yesterday':
    case 'вчера':
      date = offsetDays(1);
      break;
    case 'week':
    case 'неделя':
    case '7d':
    case '1w':
      date = offsetDays(7);
      break;
    case 'month':
    case 'месяц':
    case '30d':
    case '1mo':
      date = offsetDays(30);
      break;
    case 'year':
    case 'год':
    case '365d':
    case '1y':
      date = offsetDays(365);
      break;
    default: {
      const relative = /^(\d+)(d|w|mo|m|y)$/.exec(lowered);
      if (relative) {
        const count = Number(relative[1]);
        const days =
          relative[2] === 'd'
            ? count
            : relative[2] === 'w'
              ? count * 7
              : relative[2] === 'y'
                ? count * 365
                : count * 30;
        date = offsetDays(days);
      } else {
        date = parseAbsoluteDate(lowered);
      }
    }
  }

  if (!date || Number.isNaN(date.getTime())) {
    return null;
  }
  if (endOfDay) {
    date.setHours(23, 59, 59, 999);
  }
  return date.getTime();
}

function parseAbsoluteDate(value: string): Date | null {
  for (const pattern of [
    /^(\d{4})-(\d{2})-(\d{2})$/,
    /^(\d{2})\.(\d{2})\.(\d{4})$/,
    /^(\d{4})(\d{2})(\d{2})$/,
  ]) {
    const match = pattern.exec(value);
    if (!match) {
      continue;
    }
    const [, first, second, third] = match;
    // `дд.мм.гггг` и `гггг-мм-дд` различаются порядком групп.
    const year = first.length === 4 ? Number(first) : Number(third);
    const month = first.length === 4 ? Number(second) : Number(second);
    const day = first.length === 4 ? Number(third) : Number(first);
    const date = new Date(year, month - 1, day);
    if (!Number.isNaN(date.getTime())) {
      return date;
    }
  }
  const millis = Number(value);
  return Number.isFinite(millis) && millis > 0 ? new Date(millis) : null;
}

/** Только текстовые слова запроса — для мгновенной фильтрации списка в памяти. */
export function textTerms(parsed: ParsedQuery): { include: string[]; exclude: string[] } {
  const include: string[] = [];
  const exclude: string[] = [];
  for (const token of parsed.text) {
    const value = (token.kind === 'prefix' ? token.word : token.phrase).toLowerCase();
    if (token.negated) {
      exclude.push(value);
    } else {
      include.push(value);
    }
  }
  return { include, exclude };
}

/**
 * Быстрая проверка заметки на соответствие запросу (без участия ядра).
 *
 * Используется для фильтрации уже загруженной страницы и для подсветки;
 * настоящий поиск по всей библиотеке выполняет Rust через FTS5.
 */
export function matchesLocally(
  haystack: {
    title: string;
    excerpt: string;
    tags: string[];
    pinned: boolean;
    archived: boolean;
    encrypted: boolean;
  },
  parsed: ParsedQuery,
): boolean {
  const { include, exclude } = textTerms(parsed);
  const text = `${haystack.title}\n${haystack.excerpt}`.toLowerCase();
  if (include.length > 0 && !include.every((term) => text.includes(term))) {
    return false;
  }
  if (exclude.some((term) => text.includes(term))) {
    return false;
  }
  return parsed.filters.every((filter) => {
    switch (filter.kind) {
      case 'tag': {
        const present = haystack.tags.some((tag) => tag.toLowerCase() === filter.name);
        return present !== filter.negated;
      }
      case 'flag': {
        const present =
          filter.flag === 'pinned'
            ? haystack.pinned
            : filter.flag === 'archived'
              ? haystack.archived
              : filter.flag === 'encrypted'
                ? haystack.encrypted
                : filter.flag === 'untagged'
                  ? haystack.tags.length === 0
                  : filter.flag === 'empty'
                    ? !haystack.title.trim() && !haystack.excerpt.trim()
                    : false;
        return present !== filter.negated;
      }
      case 'color':
      case 'folder':
      case 'has':
      case 'after':
      case 'before':
        // Требуют данных, которых нет в кратком описании: их проверяет ядро.
        return true;
    }
  });
}

/** Подсвечивает найденные слова в тексте (для превью и сниппетов). */
export function highlightTerms(text: string, terms: string[]): string {
  if (terms.length === 0) {
    return escapeHtml(text);
  }
  const escaped = terms
    .filter(Boolean)
    .map((term) => term.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
    .join('|');
  if (!escaped) {
    return escapeHtml(text);
  }
  const pattern = new RegExp(`(${escaped})`, 'giu');
  return escapeHtml(text).replace(pattern, '<mark class="search-hit">$1</mark>');
}

/** Минимальное экранирование HTML. */
export function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

/**
 * Превращает сниппет ядра (с маркерами \u0001/\u0002) в безопасный HTML.
 *
 * Ядро помечает совпадения управляющими символами, чтобы не зависеть от
 * выбранного в UI маркера; здесь они заменяются на `<mark>`, а остальной
 * текст экранируется.
 */
export function renderSnippet(snippet: string): string {
  const parts = snippet.split('\u0001');
  return parts
    .map((part, index) => {
      if (index === 0) {
        return escapeHtml(part);
      }
      const close = part.indexOf('\u0002');
      if (close === -1) {
        return escapeHtml(part);
      }
      return `<mark class="search-hit">${escapeHtml(part.slice(0, close))}</mark>${escapeHtml(part.slice(close + 1))}`;
    })
    .join('');
}

/** Есть ли в запросе операторы (для подсказки «фильтры активны»). */
export function hasOperators(parsed: ParsedQuery): boolean {
  return parsed.filters.length > 0;
}
