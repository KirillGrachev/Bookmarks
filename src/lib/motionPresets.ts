import type { Variants } from 'motion/react';

/**
 * Общие пресеты появления содержимого.
 *
 * Единая семья движения для всех поверхностей: список заметок, дерево папок,
 * разделы настроек и модальные окна переключаются одинаковым кроссфейдом, а
 * непустые списки дополнительно раскрываются поколенно. Длительности и easing
 * собраны здесь, чтобы «плавно» везде означало одно и то же движение;
 * CSS-тайминги дропдаунов (`.submenu-pop` в index.css) зеркалят эти значения.
 *
 * Переключатель «Анимации» обесценивает пресеты автоматически: корневой
 * MotionConfig при выключенных анимациях ставит duration 0, а класс
 * `.disable-animations` обнуляет CSS-анимации.
 */

/** Кроссфейд блока содержимого: загрузка → список → пустое состояние. */
export const contentRevealVariants: Variants = {
  hidden: { opacity: 0 },
  visible: { opacity: 1, transition: { duration: 0.2, ease: 'easeOut' } },
  exit: { opacity: 0, transition: { duration: 0.12, ease: 'easeIn' } },
};

/**
 * Контейнер списка: кроссфейд целиком плюс каскад строк.
 *
 * `staggerChildren` касается только монтирования строк (первая загрузка, смена
 * папки или режима): строки, домонтированные фильтром поиска поверх уже
 * показанного списка, просто занимают свои места без каскада.
 */
export const listRevealVariants: Variants = {
  hidden: { opacity: 0 },
  visible: {
    opacity: 1,
    transition: { duration: 0.2, ease: 'easeOut', delayChildren: 0.04, staggerChildren: 0.035 },
  },
  exit: { opacity: 0, transition: { duration: 0.12, ease: 'easeIn' } },
};

/** Строка списка: лёгкий подъём с проявлением, без рывка по горизонтали. */
export const listRowRevealVariants: Variants = {
  hidden: { opacity: 0, y: 8 },
  visible: { opacity: 1, y: 0, transition: { duration: 0.22, ease: 'easeOut' } },
  exit: { opacity: 0, transition: { duration: 0.12, ease: 'easeIn' } },
};

/** Модальное окно: появление из небольшого масштаба, без «прыжка» на весь экран. */
export const modalVariants: Variants = {
  hidden: { opacity: 0, scale: 0.98, y: 8 },
  visible: { opacity: 1, scale: 1, y: 0, transition: { duration: 0.18, ease: 'easeOut' } },
  exit: { opacity: 0, scale: 0.98, y: 8, transition: { duration: 0.12, ease: 'easeIn' } },
};

/** Затемнение под модальным окном. */
export const overlayVariants: Variants = {
  hidden: { opacity: 0 },
  visible: { opacity: 1, transition: { duration: 0.18, ease: 'easeOut' } },
  exit: { opacity: 0, transition: { duration: 0.12, ease: 'easeIn' } },
};
