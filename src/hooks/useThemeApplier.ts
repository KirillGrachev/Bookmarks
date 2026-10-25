import { useEffect } from 'react';

import type { AccentColor, Theme } from '@/api/contracts';

/**
 * Применяет тему, акцент и переключатель анимаций к документу.
 *
 * Всё делается классами и data-атрибутами на `<html>`, а не инлайн-стилями:
 * CSS-переменные в `index.css` переключаются одной строкой, а переход темы
 * получает плавность через временный класс `.theme-transitioning`.
 */
export function useThemeApplier(theme: Theme, accent: AccentColor, animations: boolean): void {
  useEffect(() => {
    const root = document.documentElement;

    const applyMode = (dark: boolean) => {
      // Класс ставится на <html>, потому что `@custom-variant dark` в index.css
      // определён как `.dark *` — на body он бы не сработал для вложенных узлов.
      root.classList.toggle('dark', dark);
      root.style.colorScheme = dark ? 'dark' : 'light';
    };

    const media = window.matchMedia('(prefers-color-scheme: dark)');
    if (theme === 'system') {
      applyMode(media.matches);
      const listener = (event: MediaQueryListEvent) => applyMode(event.matches);
      media.addEventListener('change', listener);
      return () => media.removeEventListener('change', listener);
    }
    applyMode(theme === 'dark');
    return undefined;
  }, [theme]);

  useEffect(() => {
    document.documentElement.dataset.accent = accent;
  }, [accent]);

  useEffect(() => {
    document.body.classList.toggle('disable-animations', !animations);
  }, [animations]);

  // Плавный переход при смене темы: класс живёт ровно столько, сколько длится
  // CSS-переход (500ms), иначе анимация применялась бы к любым изменениям.
  useEffect(() => {
    const root = document.documentElement;
    root.classList.add('theme-transitioning');
    const timer = window.setTimeout(() => root.classList.remove('theme-transitioning'), 520);
    return () => {
      window.clearTimeout(timer);
      root.classList.remove('theme-transitioning');
    };
  }, [theme, accent]);
}
