import { cn } from '@/lib/utils';

interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  ariaLabel: string;
  /** Акцентный цвет во включённом состоянии (по умолчанию — primary). */
  activeClassName?: string;
  className?: string;
}

/** Доступный тумблер (`role="switch"`) — один компонент вместо копий разметки. */
export function Switch({ checked, onChange, disabled, ariaLabel, activeClassName, className }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        'relative inline-flex h-6 w-11 shrink-0 cursor-pointer items-center rounded-full border',
        'transition-colors duration-300 outline-none focus-visible:ring-2 focus-visible:ring-ring',
        checked ? cn('border-primary', activeClassName ?? 'bg-primary') : 'border-border bg-input',
        disabled && 'cursor-not-allowed opacity-50',
        className,
      )}
    >
      <span
        className={cn(
          'inline-block size-[18px] rounded-full bg-white shadow transition-transform duration-300',
          checked ? 'translate-x-[21px]' : 'translate-x-[3px]',
        )}
      />
    </button>
  );
}
