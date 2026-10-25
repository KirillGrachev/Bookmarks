import { describe, expect, it } from 'vitest';

import {
  escapeHtml,
  highlightTerms,
  matchesLocally,
  parseDate,
  parseQuery,
  renderSnippet,
  textTerms,
} from '../searchQuery';

describe('parseQuery', () => {
  it('разбирает обычные слова как префиксные токены', () => {
    const parsed = parseQuery('отчёт закупки');
    expect(parsed.text).toHaveLength(2);
    expect(parsed.filters).toHaveLength(0);
  });

  it('поддерживает фразы в кавычках', () => {
    const parsed = parseQuery('"итоги года" 2026');
    expect(parsed.text[0]).toEqual({ kind: 'phrase', phrase: 'итоги года', negated: false });
    expect(parsed.text[1]).toEqual({ kind: 'prefix', word: '2026', negated: false });
  });

  it('ведущий минус означает исключение', () => {
    const { include, exclude } = textTerms(parseQuery('отчёт -черновик'));
    expect(include).toEqual(['отчёт']);
    expect(exclude).toEqual(['черновик']);
  });

  it('извлекает операторы из текста', () => {
    const parsed = parseQuery('встреча tag:работа folder:Проекты is:pinned has:task color:red');
    expect(parsed.text).toHaveLength(1);
    const kinds = parsed.filters.map((filter) => filter.kind).sort();
    expect(kinds).toEqual(['color', 'flag', 'folder', 'has', 'tag']);
  });

  it('короткая форма #тег работает как tag:', () => {
    const parsed = parseQuery('#Работа');
    expect(parsed.text).toHaveLength(0);
    expect(parsed.filters).toEqual([{ kind: 'tag', name: 'работа', negated: false }]);
  });

  it('неизвестный оператор остаётся словом', () => {
    const parsed = parseQuery('author:pushkin');
    expect(parsed.text).toHaveLength(1);
    expect(parsed.filters).toHaveLength(0);
  });

  it('неизвестный флаг игнорируется, а не ломает запрос', () => {
    const parsed = parseQuery('текст is:красиво');
    expect(parsed.filters).toHaveLength(0);
    expect(parsed.text.map((token) => token.kind)).toContain('prefix');
  });

  it('пустой запрос не даёт токенов', () => {
    for (const raw of ['', '   ', '\t\n']) {
      const parsed = parseQuery(raw);
      expect(parsed.text).toHaveLength(0);
      expect(parsed.filters).toHaveLength(0);
    }
  });
});

describe('parseDate', () => {
  it('понимает ISO, dd.mm.yyyy и относительные значения', () => {
    expect(parseDate('2026-01-31', false)).not.toBeNull();
    expect(parseDate('31.01.2026', false)).not.toBeNull();
    expect(parseDate('20260131', false)).not.toBeNull();
    expect(parseDate('today', false)).not.toBeNull();
    expect(parseDate('7d', false)).not.toBeNull();
    expect(parseDate('сегодня', false)).not.toBeNull();
  });

  it('конец суток позже начала тех же суток', () => {
    const start = parseDate('2026-01-31', false)!;
    const end = parseDate('2026-01-31', true)!;
    expect(end).toBeGreaterThan(start);
  });

  it('мусор возвращает null', () => {
    expect(parseDate('не-дата', false)).toBeNull();
    expect(parseDate('', false)).toBeNull();
  });
});

describe('matchesLocally', () => {
  const note = {
    title: 'Совещание по релизу',
    excerpt: 'обсудили сроки и задачи',
    tags: ['работа', 'встречи'],
    pinned: true,
    archived: false,
    encrypted: false,
  };

  it('находит по слову из заголовка и текста', () => {
    expect(matchesLocally(note, parseQuery('релиз'))).toBe(true);
    expect(matchesLocally(note, parseQuery('сроки'))).toBe(true);
  });

  it('исключение отсекает заметку', () => {
    expect(matchesLocally(note, parseQuery('релиз -сроки'))).toBe(false);
  });

  it('фильтр по тегу и флагу', () => {
    expect(matchesLocally(note, parseQuery('tag:работа'))).toBe(true);
    expect(matchesLocally(note, parseQuery('tag:личное'))).toBe(false);
    expect(matchesLocally(note, parseQuery('is:pinned'))).toBe(true);
    expect(matchesLocally(note, parseQuery('-is:pinned'))).toBe(false);
    expect(matchesLocally(note, parseQuery('is:untagged'))).toBe(false);
  });

  it('регистр не важен', () => {
    expect(matchesLocally(note, parseQuery('РЕЛИЗ'))).toBe(true);
    expect(matchesLocally(note, parseQuery('TAG:Работа'))).toBe(true);
  });
});

describe('renderSnippet', () => {
  it('заменяет маркеры ядра на <mark> и экранирует остальное', () => {
    const html = renderSnippet('до \u0001совпадение\u0002 после <script>');
    expect(html).toContain('<mark class="search-hit">совпадение</mark>');
    expect(html).toContain('&lt;script&gt;');
    expect(html).not.toContain('<script>');
  });

  it('текст без маркеров просто экранируется', () => {
    expect(renderSnippet('a & b')).toBe('a &amp; b');
  });
});

describe('highlightTerms', () => {
  it('подсвечивает совпадения', () => {
    expect(highlightTerms('Отчёт за квартал', ['отчёт'])).toBe(
      '<mark class="search-hit">Отчёт</mark> за квартал',
    );
  });

  it('экранирует HTML в исходном тексте', () => {
    expect(highlightTerms('<b>текст</b>', ['текст'])).toBe(
      '&lt;b&gt;<mark class="search-hit">текст</mark>&lt;/b&gt;',
    );
  });

  it('пустой список терминов ничего не меняет', () => {
    expect(highlightTerms('просто текст', [])).toBe('просто текст');
  });

  it('спецсимволы regex в термине не ломают подсветку', () => {
    expect(() => highlightTerms('a+b', ['a+b'])).not.toThrow();
    expect(highlightTerms('a+b', ['a+b'])).toContain('<mark');
  });
});

describe('escapeHtml', () => {
  it('экранирует все опасные символы', () => {
    expect(escapeHtml(`<>"'&`)).toBe('&lt;&gt;&quot;&#39;&amp;');
  });
});
