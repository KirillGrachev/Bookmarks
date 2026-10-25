import type { ComponentProps } from 'react';
import type { LucideIcon } from 'lucide-react';

import { cn } from '@/lib/utils';

interface IconButtonProps extends ComponentProps<'button'> {
  icon: LucideIcon;
  /** Подсказка и aria-label одновременно: кнопка без текста обязана быть подписана. */
  label: string;
  size?: 'sm' | 'md' | 'lg';
  active?: boolean;
  danger?: boolean;
}

const SIZES = {
  sm: 'h-7 w-7 rounded-md [&_svg]:size-3.5',
  md: 'h-9 w-9 rounded-lg [&_svg]:size-[18px]',
  lg: 'h-11 w-11 rounded-full [&_svg]:size-6',
} as const;

/**
 * Иконочная кнопка с единым стилем нажатия и ховера.
 *
 * Осознанно без сдвига при нажатии (`active:translate-y`): в модальных окнах
 * дёргающийся крестик выглядит как дефект, а не как отклик.
 */
export function IconButton({
  icon: Icon,
  label,
  size = 'md',
  active = false,
  danger = false,
  className,
  type = 'button',
  ...props
}: IconButtonProps) {
  return (
    <button
      type={type}
      title={label}
      aria-label={label}
      aria-pressed={active || undefined}
      className={cn(
        'inline-flex shrink-0 cursor-pointer items-center justify-center border border-transparent',
        'text-muted-foreground transition-colors duration-200 outline-none',
        'hover:bg-surface-hover hover:text-foreground',
        'focus-visible:ring-2 focus-visible:ring-ring',
        'disabled:pointer-events-none disabled:opacity-40',
        SIZES[size],
        active && 'bg-primary/10 text-primary hover:bg-primary/15 hover:text-primary',
        danger && 'hover:bg-danger/10 hover:text-danger',
        className,
      )}
      {...props}
    >
      <Icon className="pointer-events-none" aria-hidden />
    </button>
  );
}
