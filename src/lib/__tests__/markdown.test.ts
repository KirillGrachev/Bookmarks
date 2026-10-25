import { describe, expect, it } from 'vitest';

import {
  clearMarkdownCache,
  countWords,
  prefixLines,
  readingTimeMinutes,
  renderMarkdown,
  toggleTaskAtLine,
  wrapSelection,
} from '../markdown';

describe('renderMarkdown', () => {
  it('рендерит базовые конструкции', () => {
    const html = renderMarkdown('# Заголовок\n\n**жирный** и `код`\n\n- пункт');
    expect(html).toContain('<h1');
    expect(html).toContain('<strong>жирный</strong>');
    expect(html).toContain('<code>код</code>');
    expect(html).toContain('<li');
  });

  it('вырезает скрипты и обработчики событий', () => {
    clearMarkdownCache();
    const html = renderMarkdown('<img src=x onerror="alert(1)">\n\n<script>alert(2)</script>');
    expect(html).not.toContain('<script');
    expect(html).not.toContain('onerror');
  });

  it('не пропускает javascript:-ссылки', () => {
    clearMarkdownCache();
    const html = renderMarkdown('[клик](javascript:alert(1))');
    expect(html).not.toContain('javascript:');
  });

  it('таблицы и задачи (GFM) работают', () => {
    clearMarkdownCache();
    const html = renderMarkdown('| a | b |\n|---|---|\n| 1 | 2 |\n\n- [ ] задача');
    expect(html).toContain('<table');
    expect(html).toContain('type="checkbox"');
  });

  it('кэширует повторный рендер того же текста', () => {
    clearMarkdownCache();
    const first = renderMarkdown('одинаковый текст');
    const second = renderMarkdown('одинаковый текст');
    expect(first).toBe(second);
  });
});

describe('wrapSelection', () => {
  it('оборачивает выделенное', () => {
    const result = wrapSelection('привет мир', 0, 6, '**', '**');
    expect(result.value).toBe('**привет** мир');
    expect(result.selectionStart).toBe(2);
    expect(result.selectionEnd).toBe(8);
  });

  it('повторное нажатие снимает разметку', () => {
    const result = wrapSelection('**привет** мир', 2, 8, '**', '**');
    expect(result.value).toBe('привет мир');
    expect(result.selectionStart).toBe(0);
    expect(result.selectionEnd).toBe(6);
  });

  it('работает с пустым выделением', () => {
    const result = wrapSelection('текст', 2, 2, '`', '`');
    expect(result.value).toBe('те``кст');
  });
});

describe('prefixLines', () => {
  it('добавляет префикс к каждой строке блока', () => {
    const result = prefixLines('строка 1\nстрока 2', 0, 16, '- ');
    expect(result.value).toBe('- строка 1\n- строка 2');
  });

  it('повторное нажатие убирает префикс', () => {
    const result = prefixLines('- строка 1\n- строка 2', 0, 22, '- ');
    expect(result.value).toBe('строка 1\nстрока 2');
  });

  it('не трогает пустые строки', () => {
    const result = prefixLines('a\n\nb', 0, 4, '> ');
    expect(result.value).toBe('> a\n\n> b');
  });
});

describe('toggleTaskAtLine', () => {
  it('переключает состояние задачи', () => {
    expect(toggleTaskAtLine('- [ ] купить', 0)).toBe('- [x] купить');
    expect(toggleTaskAtLine('- [x] купить', 0)).toBe('- [ ] купить');
    expect(toggleTaskAtLine('- [X] купить', 0)).toBe('- [ ] купить');
  });

  it('обычная строка становится задачей', () => {
    expect(toggleTaskAtLine('купить', 0)).toBe('- [ ] купить');
    expect(toggleTaskAtLine('- купить', 0)).toBe('- [ ] купить');
  });

  it('выход за границы текста безопасен', () => {
    expect(toggleTaskAtLine('текст', 9)).toBe('текст');
    expect(toggleTaskAtLine('текст', -1)).toBe('текст');
  });
});

describe('countWords и readingTimeMinutes', () => {
  it('считает слова без разметки', () => {
    expect(countWords('')).toBe(0);
    expect(countWords('раз два три')).toBe(3);
    expect(countWords('# Заголовок\n\n- пункт **один**')).toBe(3);
    expect(countWords('hello_world 42')).toBe(2);
  });

  it('время чтения не меньше минуты', () => {
    expect(readingTimeMinutes('коротко')).toBe(1);
    expect(readingTimeMinutes('слово '.repeat(400))).toBe(2);
  });
});
