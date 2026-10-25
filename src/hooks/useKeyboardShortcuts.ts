import { useEffect, useRef } from 'react';

/** Горячая клавиша в терминах кодов клавиш (не зависит от раскладки). */
export interface Shortcut {
  /** `KeyN`, `Slash`, `Digit1`… — берём из `KeyboardEvent.code`. */
  code: string;
  ctrl?: boolean;
  shift?: boolean;
  alt?: boolean;
  /** Разрешить срабатывание, когда фокус в поле ввода. */
  allowInInput?: boolean;
}

export interface ShortcutBinding {
  shortcut: Shortcut;
  action: () => void;
  /** Человекочитаемое имя — для экрана со списком горячих клавиш. */
  id: string;
}

function isEditable(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  const tag = target.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || target.isContentEditable;
}

/**
 * Глобальные горячие клавиши окна.
 *
 * Один слушатель на весь набор (а не по одному на сочетание): меньше подписок,
 * предсказуемый порядок и единое правило «в полях ввода не срабатываем», кроме
 * явно разрешённых (Escape, Ctrl+A в редакторе).
 */
export function useKeyboardShortcuts(bindings: ShortcutBinding[]): void {
  // Храним привязки в ref, чтобы не переподписывать слушатель на каждый рендер.
  const bindingsRef = useRef(bindings);
  bindingsRef.current = bindings;

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      for (const binding of bindingsRef.current) {
        const { code, ctrl, shift, alt, allowInInput } = binding.shortcut;
        if (event.code !== code) {
          continue;
        }
        if (Boolean(ctrl) !== (event.ctrlKey || event.metaKey)) {
          continue;
        }
        if (Boolean(shift) !== event.shiftKey) {
          continue;
        }
        if (Boolean(alt) !== event.altKey) {
          continue;
        }
        if (!allowInInput && isEditable(event.target)) {
          continue;
        }
        event.preventDefault();
        binding.action();
        return;
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);
}

/** Читаемое представление сочетания для подсказок (Ctrl+K, Ctrl+Shift+F). */
export function formatShortcut(shortcut: Shortcut, macLike = false): string {
  const parts: string[] = [];
  if (shortcut.ctrl) {
    parts.push(macLike ? '⌘' : 'Ctrl');
  }
  if (shortcut.shift) {
    parts.push(macLike ? '⇧' : 'Shift');
  }
  if (shortcut.alt) {
    parts.push(macLike ? '⌥' : 'Alt');
  }
  const key = shortcut.code
    .replace(/^Key/, '')
    .replace(/^Digit/, '')
    .replace(/^Arrow/, '')
    .replace(/^Numpad/, 'Num ');
  parts.push(key.length === 1 ? key.toUpperCase() : key);
  return parts.join('+');
}
