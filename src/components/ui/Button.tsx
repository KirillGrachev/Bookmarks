import type { ButtonHTMLAttributes, ReactNode } from 'react';
import { cva, type VariantProps } from 'class-variance-authority';

import { cn } from '@/lib/utils';

const button = cva(
  'inline-flex cursor-pointer items-center justify-center gap-2 whitespace-nowrap rounded-lg font-medium ' +
    'transition-colors duration-200 outline-none focus-visible:ring-2 focus-visible:ring-ring ' +
    'disabled:pointer-events-none disabled:opacity-40 select-none',
  {
    variants: {
      variant: {
        primary: 'bg-primary text-primary-foreground hover:bg-primary-hover shadow-sm active:shadow-none',
        secondary: 'bg-muted text-foreground hover:bg-surface-hover border border-border',
        ghost: 'text-muted-foreground hover:bg-surface-hover hover:text-foreground',
        danger: 'bg-danger text-white hover:brightness-110',
        outline: 'border border-border text-foreground hover:bg-surface-hover',
      },
      size: {
        sm: 'h-8 px-3 text-[0.8rem]',
        md: 'h-10 px-4 text-[0.85rem]',
        lg: 'h-11 px-5 text-[0.9rem]',
        icon: 'h-9 w-9',
      },
    },
    defaultVariants: { variant: 'primary', size: 'md' },
  },
);

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement>, VariantProps<typeof button> {
  children?: ReactNode;
}

/** Кнопка приложения: четыре варианта, чтобы в интерфейсе не плодились стили. */
export function Button({ className, variant, size, type = 'button', ...props }: ButtonProps) {
  return <button type={type} className={cn(button({ variant, size }), className)} {...props} />;
}
