import { useEffect, useState } from 'react';
import { AnimatePresence, MotionConfig, motion } from 'motion/react';
import { Toaster } from 'sonner';
import { Loader2 } from 'lucide-react';

import { TitleBar } from '@/components/TitleBar';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import { NotesPage } from '@/features/notes/NotesPage';
import { SettingsPage } from '@/features/settings/SettingsPage';
import { useI18n } from '@/hooks/useI18n';
import { useThemeApplier } from '@/hooks/useThemeApplier';
import { useAppStore } from '@/store/useAppStore';

/**
 * Корневой компонент: настройки, тема, оболочка окна и переключение экранов.
 *
 * Splash держится до загрузки настроек, иначе первый кадр рисуется со
 * значениями по умолчанию и «прыгает» в тему пользователя на глазах.
 */
export default function App() {
  const { t } = useI18n();
  const settings = useAppStore((state) => state.settings);
  const settingsLoaded = useAppStore((state) => state.settingsLoaded);
  const loadSettings = useAppStore((state) => state.loadSettings);
  const settingsOpen = useAppStore((state) => state.settingsOpen);
  const syncLanguage = useAppStore((state) => state.setLanguage);
  const [showSplash, setShowSplash] = useState(true);

  const { theme, accent, animations } = settings.interface;
  useThemeApplier(theme, accent, animations);

  useEffect(() => {
    void loadSettings();
  }, [loadSettings]);

  // Язык интерфейса следует за настройкой (единый источник правды — settings).
  useEffect(() => {
    if (settingsLoaded) {
      syncLanguage(settings.interface.language);
    }
  }, [settings.interface.language, settingsLoaded, syncLanguage]);

  useEffect(() => {
    if (!settingsLoaded) {
      return undefined;
    }
    const timer = window.setTimeout(() => setShowSplash(false), 220);
    return () => window.clearTimeout(timer);
  }, [settingsLoaded]);

  return (
    <MotionConfig
      // reducedMotion привязан к переключателю приложения, а не к ОС: явный
      // выбор пользователя важнее системной настройки.
      transition={animations ? undefined : { duration: 0 }}
      reducedMotion={animations ? 'never' : 'always'}
    >
      <AnimatePresence mode="wait">
        {showSplash ? (
          <Splash key="splash" />
        ) : (
          <motion.div
            key="app"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ duration: 0.25, ease: 'easeOut' }}
            className="relative flex h-[100dvh] w-full flex-col overflow-hidden bg-background font-sans text-foreground select-none"
          >
            <ErrorBoundary>
              <header role="banner">
                <TitleBar title="Bookmarks" subtitle={currentNoteTitle()} t={t} />
              </header>

              <div className="relative flex min-h-0 flex-1">
                <NotesPage />
                {settingsOpen && <SettingsPage />}
              </div>

              <Toaster
                position="bottom-right"
                theme={theme === 'system' ? 'system' : theme}
                duration={4000}
                closeButton
                richColors
                visibleToasts={3}
                toastOptions={{
                  className:
                    'rounded-lg border border-border bg-surface-raised p-4 text-[0.83rem] font-medium text-foreground shadow-2xl',
                  style: { fontFamily: 'inherit' },
                }}
              />
            </ErrorBoundary>
          </motion.div>
        )}
      </AnimatePresence>
    </MotionConfig>
  );
}

/** Заголовок открытой заметки для строки заголовка окна. */
function currentNoteTitle(): string | undefined {
  const note = useAppStore.getState().openNote;
  const title = note?.title.trim();
  return title ? title : undefined;
}

function Splash() {
  return (
    <motion.div
      key="splash"
      exit={{ opacity: 0 }}
      transition={{ duration: 0.2, ease: 'easeOut' }}
      className="grid h-[100dvh] w-full place-items-center bg-background"
      role="status"
      aria-live="polite"
    >
      <div className="flex flex-col items-center gap-3">
        <span className="grid size-11 place-items-center rounded-xl bg-[image:var(--primary-gradient)] text-lg font-bold text-white shadow-lg">
          B
        </span>
        <Loader2 className="size-4 animate-spin text-muted-foreground" aria-hidden />
      </div>
    </motion.div>
  );
}
