import DOMPurify from 'dompurify';
import { marked } from 'marked';

/**
 * Рендер Markdown в безопасный HTML.
 *
 * Заметки — это пользовательский ввод, а превью вставляется через
 * `dangerouslySetInnerHTML`, поэтому санитизация обязательна: без неё
 * `<img onerror=…>` из чужого импортированного файла выполнился бы в окне
 * приложения, у которого есть доступ к IPC.
 *
 * Настройка `marked` одна на модуль: повторная инициализация на каждый рендер
 * дала бы заметную просадку при вводе текста.
 */
marked.setOptions({
  gfm: true,
  breaks: true,
  async: false,
});

DOMPurify.setConfig({
  // `input` разрешён только ради чекбоксов списков задач (GFM): marked
  // выдаёт их как <input type="checkbox" disabled>, а без них список дел
  // в превью превращается в набор квадратов.
  FORBID_ATTR: ['target', 'onerror', 'onload', 'onclick', 'oninput', 'value', 'srcdoc', 'formaction'],
  FORBID_TAGS: ['style', 'form', 'button', 'iframe', 'object', 'embed', 'script', 'textarea', 'select'],
  ALLOW_DATA_ATTR: false,
});

/** Кэш последних рендеров: редактор перерисовывает превью на каждое нажатие. */
const cache = new Map<string, string>();
const CACHE_LIMIT = 32;

/** Преобразует Markdown в sanitized HTML (с кэшем по исходному тексту). */
export function renderMarkdown(markdown: string): string {
  const cached = cache.get(markdown);
  if (cached !== undefined) {
    return cached;
  }
  const html = DOMPurify.sanitize(marked.parse(markdown, { async: false }) as string, {
    RETURN_TRUSTED_TYPE: false,
  });
  if (cache.size >= CACHE_LIMIT) {
    // Map сохраняет порядок вставки: удаляем самую старую запись.
    const oldest = cache.keys().next().value;
    if (oldest !== undefined) {
      cache.delete(oldest);
    }
  }
  cache.set(markdown, html);
  return html;
}

/** Очищает кэш (смена темы не требует, но тесты — да). */
export function clearMarkdownCache(): void {
  cache.clear();
}

/**
 * Вставляет Markdown-обёртку вокруг выделения в textarea.
 *
 * Возвращает новое значение и диапазон выделения, чтобы редактор не терял
 * курсор: пересоздавать выделение после каждой кнопки — заметно на глаз.
 */
export interface WrapResult {
  value: string;
  selectionStart: number;
  selectionEnd: number;
}

export function wrapSelection(
  value: string,
  start: number,
  end: number,
  before: string,
  after: string,
): WrapResult {
  const selected = value.slice(start, end);
  const alreadyWrapped =
    value.slice(Math.max(0, start - before.length), start) === before &&
    value.slice(end, end + after.length) === after;

  if (alreadyWrapped) {
    // Повторное нажатие снимает разметку — иначе жирный текст приходится
    // удалять руками, что в заметочнике ощущается как баг.
    const next = value.slice(0, start - before.length) + selected + value.slice(end + after.length);
    return {
      value: next,
      selectionStart: start - before.length,
      selectionEnd: end - before.length,
    };
  }

  const next = value.slice(0, start) + before + selected + after + value.slice(end);
  return {
    value: next,
    selectionStart: start + before.length,
    selectionEnd: end + before.length,
  };
}

/** Вставляет префикс в начало каждой выделенной строки (списки, цитаты, заголовки). */
export function prefixLines(value: string, start: number, end: number, prefix: string): WrapResult {
  const lineStart = value.lastIndexOf('\n', Math.max(0, start - 1)) + 1;
  const lineEnd = value.indexOf('\n', end);
  const blockEnd = lineEnd === -1 ? value.length : lineEnd;
  const block = value.slice(lineStart, blockEnd);

  const allPrefixed = block.split('\n').every((line) => line.trim() === '' || line.startsWith(prefix));
  const nextBlock = block
    .split('\n')
    .map((line) => {
      if (line.trim() === '') {
        return line;
      }
      return allPrefixed ? line.slice(prefix.length) : `${prefix}${line}`;
    })
    .join('\n');

  const delta = nextBlock.length - block.length;
  return {
    value: value.slice(0, lineStart) + nextBlock + value.slice(blockEnd),
    selectionStart: Math.max(lineStart, start + delta),
    selectionEnd: Math.max(lineStart, end + delta),
  };
}

/**
 * Пункты списка задач: `- [ ]` / `- [x]`.
 *
 * Отдельная функция, потому что чекбокс в превью кликабелен и должен менять
 * исходный текст — это самая частая операция в списках дел.
 */
export function toggleTaskAtLine(value: string, lineIndex: number): string {
  const lines = value.split('\n');
  if (lineIndex < 0 || lineIndex >= lines.length) {
    return value;
  }
  const line = lines[lineIndex];
  const match = /^(\s*(?:[-*+]|\d+[.)])\s+)\[( |x|X)\](.*)$/.exec(line);
  if (!match) {
    // Обычная строка становится незавершенной задачей — так работает
    // клик по чекбоксу в большинстве заметочников.
    lines[lineIndex] = /^\s*(?:[-*+]|\d+[.)])\s+/.test(line)
      ? line.replace(/^(\s*(?:[-*+]|\d+[.)])\s+)/, '$1[ ] ')
      : `- [ ] ${line}`;
    return lines.join('\n');
  }
  const [, lead, state, rest] = match;
  lines[lineIndex] = `${lead}[${state === ' ' ? 'x' : ' '}]${rest}`;
  return lines.join('\n');
}

/** Число слов — для статусной строки редактора (совпадает с расчётом ядра). */
export function countWords(text: string): number {
  const matches = text.match(/[\p{L}\p{N}_]+/gu);
  return matches ? matches.length : 0;
}

/** Время чтения в минутах (200 слов в минуту). */
export function readingTimeMinutes(text: string): number {
  return Math.max(1, Math.round(countWords(text) / 200));
}
