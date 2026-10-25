import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach, vi } from 'vitest';

import { setBackend } from '@/api/backend';
import { DemoBackend } from '@/api/demo';

// Каждый тест работает с чистым DOM и свежим демо-хранилищем: иначе состояние
// из предыдущего теста незаметно влияет на следующий, и падение выглядит
// как случайное.
afterEach(() => {
  cleanup();
  localStorage.clear();
});

// Демо-бэкенд по умолчанию; тесты, которым нужен мок, подменяют его через
// setBackend() в самом тесте.
setBackend(new DemoBackend());

// matchMedia в jsdom отсутствует, а тема «как в системе» его использует.
if (typeof window !== 'undefined' && !window.matchMedia) {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
}

// scrollTo не реализован в jsdom, а виртуализированные списки его вызывают.
if (typeof window !== 'undefined' && !window.scrollTo) {
  window.scrollTo = vi.fn() as unknown as typeof window.scrollTo;
}
