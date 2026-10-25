import { useCallback, useEffect, useState } from 'react';

/**
 * Значение с задержкой.
 *
 * Поиск и автосохранение дёргают ядро на каждое нажатие; без debounce это
 * десятки запросов в секунду и видимая задержка ввода. Задержка совпадает с
 * настройкой автосохранения, поэтому поведение предсказуемо.
 */
export function useDebouncedValue<T>(value: T, delayMs: number): T {
  const [debounced, setDebounced] = useState(value);

  useEffect(() => {
    const timer = setTimeout(() => setDebounced(value), delayMs);
    return () => clearTimeout(timer);
  }, [value, delayMs]);

  return debounced;
}

/**
 * Отложенный вызов с отменой и «доводкой» при размонтировании.
 *
 * В отличие от простого debounce, возвращает `flush()`: перед закрытием окна
 * или переключением заметки несохранённый текст нужно записать сразу, а не
 * потерять вместе с таймером.
 */
export function useDebouncedAction(action: () => void | Promise<void>, delayMs: number) {
  const [pending, setPending] = useState(false);

  useEffect(() => {
    if (!pending) {
      return undefined;
    }
    const timer = setTimeout(() => {
      setPending(false);
      void action();
    }, delayMs);
    return () => clearTimeout(timer);
  }, [pending, delayMs, action]);

  const schedule = useCallback(() => setPending(true), []);
  const cancel = useCallback(() => setPending(false), []);
  const flush = useCallback(() => {
    if (pending) {
      setPending(false);
      void action();
    }
  }, [pending, action]);

  return { schedule, cancel, flush, pending };
}
