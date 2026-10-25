import { useEffect, useState } from 'react';
import { Copy as RestoreIcon, Minus, Square, X } from 'lucide-react';

import { isTauri } from '@/api/backend';
import { cn } from '@/lib/utils';
import type { Translator } from '@/lib/i18n';

const BUTTON_CLASS =
  'flex h-full w-[46px] cursor-pointer items-center justify-center text-foreground/40 transition-colors ' +
  'hover:bg-foreground/5 hover:text-foreground outline-none ' +
  'focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-inset';

interface TitleBarProps {
  title: string;
  /** Имя открытой заметки — показывается рядом с названием приложения. */
  subtitle?: string;
  t: Translator;
}

/**
 * Кастомная строка заголовка (окно без системной рамки).
 *
 * Управление окном идёт через штатный JS API `@tauri-apps/api/window`,
 * самописные Rust-команды для minimize/maximize/close не нужны. Иконка
 * «развернуть/восстановить» синхронизируется с реальным состоянием окна через
 * подписку на resize — иначе после перетаскивания за край она показывает
 * неверное действие.
 */
export function TitleBar({ title, subtitle, t }: TitleBarProps) {
  const desktop = isTauri();
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!desktop) {
      return undefined;
    }
    let unlisten: (() => void) | undefined;
    let disposed = false;

    void (async () => {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      const current = getCurrentWindow();
      try {
        setMaximized(await current.isMaximized());
        const fn = await current.onResized(() => {
          void current
            .isMaximized()
            .then(setMaximized)
            .catch(() => {});
        });
        if (disposed) {
          fn();
        } else {
          unlisten = fn;
        }
      } catch (error) {
        console.warn('[titlebar] window state tracking failed', error);
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [desktop]);

  const logo = (
    <span className="mr-2 grid size-4 shrink-0 place-items-center rounded-[4px] bg-[image:var(--primary-gradient)] text-[9px] font-bold text-white">
      B
    </span>
  );

  if (!desktop) {
    // Браузерный демо-режим: строка заголовка без оконных кнопок.
    return (
      <div className="relative z-[60] flex h-8 shrink-0 select-none items-center justify-between border-b border-border bg-surface">
        <div className="flex h-full min-w-0 flex-1 items-center pl-3">
          {logo}
          <span className="truncate text-[0.72rem] font-semibold tracking-wide text-foreground/85 uppercase">
            {title}
          </span>
          {subtitle && (
            <span className="ml-2 truncate text-[0.72rem] text-muted-foreground">— {subtitle}</span>
          )}
        </div>
        <span className="pr-3 text-[0.68rem] text-muted-foreground/70">demo</span>
      </div>
    );
  }

  return (
    <div className="relative z-[60] flex h-8 shrink-0 select-none items-center justify-between border-b border-border bg-surface">
      <div data-tauri-drag-region className="flex h-full min-w-0 flex-1 cursor-default items-center pl-3">
        <span data-tauri-drag-region>{logo}</span>
        <span
          data-tauri-drag-region
          className="pointer-events-none truncate text-[0.72rem] font-semibold tracking-wide text-foreground/85 uppercase"
        >
          {title}
        </span>
        {subtitle && (
          <span
            data-tauri-drag-region
            className="pointer-events-none ml-2 truncate text-[0.72rem] text-muted-foreground"
          >
            — {subtitle}
          </span>
        )}
      </div>

      <WindowButtons maximized={maximized} t={t} />
    </div>
  );
}

function WindowButtons({ maximized, t }: { maximized: boolean; t: Translator }) {
  const [run, setRun] = useState<null | ((action: string) => void)>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      const current = getCurrentWindow();
      if (cancelled) {
        return;
      }
      setRun(() => (action: string) => {
        const promise =
          action === 'minimize'
            ? current.minimize()
            : action === 'toggleMaximize'
              ? current.toggleMaximize()
              : current.close();
        promise.catch((error) => console.error('[titlebar] window action failed', error));
      });
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div className="flex h-full items-center">
      <button
        type="button"
        onClick={() => run?.('minimize')}
        className={BUTTON_CLASS}
        title={t('window.minimize')}
        aria-label={t('window.minimize')}
      >
        <Minus size={14} className="pointer-events-none" aria-hidden />
      </button>
      <button
        type="button"
        onClick={() => run?.('toggleMaximize')}
        className={BUTTON_CLASS}
        title={maximized ? t('window.restore') : t('window.maximize')}
        aria-label={maximized ? t('window.restore') : t('window.maximize')}
        aria-pressed={maximized}
      >
        {maximized ? (
          <RestoreIcon size={13} className="pointer-events-none" aria-hidden />
        ) : (
          <Square size={13} className="pointer-events-none" aria-hidden />
        )}
      </button>
      <button
        type="button"
        onClick={() => run?.('close')}
        className={cn(BUTTON_CLASS, 'hover:bg-[#e81123] hover:text-white')}
        title={t('window.close')}
        aria-label={t('window.close')}
      >
        <X size={16} className="pointer-events-none" aria-hidden />
      </button>
    </div>
  );
}
