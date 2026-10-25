import { useMemo } from 'react';

import { createTranslator, type Lang, type Translator } from '@/lib/i18n';
import { useAppStore } from '@/store/useAppStore';

/**
 * Переводчик, привязанный к текущему языку настроек.
 *
 * Возвращает стабильную функцию (пересоздаётся только при смене языка),
 * поэтому её можно безопасно класть в зависимости эффектов и пропсы.
 */
export function useI18n(): { t: Translator; lang: Lang } {
  const language = useAppStore((state) => state.settings.interface.language);
  const t = useMemo(() => createTranslator(language), [language]);
  return { t, lang: language };
}
