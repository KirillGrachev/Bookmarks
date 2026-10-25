import { useEffect, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { AnimatePresence, motion } from 'motion/react';
import { X } from 'lucide-react';

import { IconButton } from '@/components/ui/IconButton';
import { modalVariants, overlayVariants } from '@/lib/motionPresets';
import { cn } from '@/lib/utils';

interface ModalProps {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: string;
  children: ReactNode;
  footer?: ReactNode;
  /** Подпись кнопки закрытия (для aria-label). */
  closeLabel: string;
  width?: string;
}

/**
 * Модальное окно приложения.
 *
 * Рендерится в портал, чтобы `overflow: hidden` основного контейнера не
 * обрезало его, а z-index не соревновался с тостами. Закрытие по Escape и клику
 * на подложку; фокус возвращается элементу, который был активен до открытия.
 */
export function Modal({
  open,
  onClose,
  title,
  description,
  children,
  footer,
  closeLabel,
  width = 'max-w-lg',
}: ModalProps) {
  const [previousFocus, setPreviousFocus] = useState<HTMLElement | null>(null);

  useEffect(() => {
    if (!open) {
      return undefined;
    }
    setPreviousFocus(document.activeElement as HTMLElement | null);
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation();
        onClose();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
      previousFocus?.focus?.();
    };
    // previousFocus намеренно не в зависимостях: возвращать фокус нужно тому
    // элементу, который был активен на момент открытия.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, onClose]);

  return createPortal(
    <AnimatePresence>
      {open && (
        <div className="fixed inset-0 z-[9990] flex items-center justify-center p-6">
          <motion.div
            className="absolute inset-0 bg-black/40 backdrop-blur-[2px]"
            variants={overlayVariants}
            initial="hidden"
            animate="visible"
            exit="exit"
            onClick={onClose}
            aria-hidden
          />
          <motion.div
            role="dialog"
            aria-modal="true"
            aria-label={title}
            variants={modalVariants}
            initial="hidden"
            animate="visible"
            exit="exit"
            className={cn(
              'relative w-full rounded-xl border border-border bg-surface-raised shadow-2xl',
              width,
            )}
          >
            <header className="flex items-start justify-between gap-4 border-b border-border px-5 py-4">
              <div className="min-w-0">
                <h2 className="truncate text-[0.95rem] font-semibold text-foreground">{title}</h2>
                {description && (
                  <p className="mt-1 text-[0.8rem] leading-relaxed text-muted-foreground">{description}</p>
                )}
              </div>
              <IconButton icon={X} label={closeLabel} size="sm" onClick={onClose} />
            </header>

            <div className="max-h-[65vh] overflow-y-auto px-5 py-4">{children}</div>

            {footer && (
              <footer className="flex items-center justify-end gap-2 border-t border-border px-5 py-3">
                {footer}
              </footer>
            )}
          </motion.div>
        </div>
      )}
    </AnimatePresence>,
    document.body,
  );
}

/** Диалог подтверждения: один компонент на все «вы уверены?». */
export function ConfirmModal({
  open,
  title,
  message,
  confirmLabel,
  cancelLabel,
  destructive = false,
  onConfirm,
  onCancel,
}: {
  open: boolean;
  title: string;
  message: string;
  confirmLabel: string;
  cancelLabel: string;
  destructive?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <Modal
      open={open}
      onClose={onCancel}
      title={title}
      closeLabel={cancelLabel}
      width="max-w-md"
      footer={
        <>
          <button
            type="button"
            onClick={onCancel}
            className="h-9 cursor-pointer rounded-lg border border-border px-4 text-[0.83rem] text-foreground transition-colors hover:bg-surface-hover"
          >
            {cancelLabel}
          </button>
          <button
            type="button"
            onClick={onConfirm}
            className={cn(
              'h-9 cursor-pointer rounded-lg px-4 text-[0.83rem] font-medium text-white transition-all',
              destructive ? 'bg-danger hover:brightness-110' : 'bg-primary hover:bg-primary-hover',
            )}
          >
            {confirmLabel}
          </button>
        </>
      }
    >
      <p className="text-[0.85rem] leading-relaxed text-foreground/85">{message}</p>
    </Modal>
  );
}
