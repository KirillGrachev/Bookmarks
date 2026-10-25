import type { InputHTMLAttributes, TextareaHTMLAttributes } from 'react';
import { forwardRef } from 'react';

import { cn } from '@/lib/utils';

const FIELD_CLASS =
  'w-full rounded-lg border border-input bg-surface px-3 py-2 text-[0.85rem] text-foreground ' +
  'placeholder:text-muted-foreground/70 transition-colors duration-200 outline-none ' +
  'hover:border-input-hover focus:border-primary focus:ring-2 focus:ring-ring/60 ' +
  'disabled:cursor-not-allowed disabled:opacity-50';

interface TextFieldProps extends InputHTMLAttributes<HTMLInputElement> {
  label?: string;
  hint?: string;
  /** Текст ошибки из ядра (`ErrorPayload.field` подсвечивает конкретное поле). */
  error?: string | null;
}

/** Однострочное поле с подписью и местом под ошибку. */
export const TextField = forwardRef<HTMLInputElement, TextFieldProps>(function TextField(
  { label, hint, error, className, id, ...props },
  ref,
) {
  const fieldId = id ?? props.name ?? label;
  return (
    <label className="block" htmlFor={fieldId}>
      {label && (
        <span className="mb-1.5 block text-[0.78rem] font-medium text-muted-foreground">{label}</span>
      )}
      <input
        ref={ref}
        id={fieldId}
        aria-invalid={error ? true : undefined}
        aria-describedby={error ? `${fieldId}-error` : hint ? `${fieldId}-hint` : undefined}
        className={cn(FIELD_CLASS, error && 'border-danger focus:border-danger', className)}
        {...props}
      />
      {error ? (
        <span id={`${fieldId}-error`} className="mt-1 block text-[0.75rem] text-danger" role="alert">
          {error}
        </span>
      ) : (
        hint && (
          <span id={`${fieldId}-hint`} className="mt-1 block text-[0.75rem] text-muted-foreground">
            {hint}
          </span>
        )
      )}
    </label>
  );
});

interface TextAreaProps extends TextareaHTMLAttributes<HTMLTextAreaElement> {
  label?: string;
}

/** Многострочное поле (описания, пароли с переносом). */
export const TextArea = forwardRef<HTMLTextAreaElement, TextAreaProps>(function TextArea(
  { label, className, id, ...props },
  ref,
) {
  const fieldId = id ?? props.name ?? label;
  return (
    <label className="block" htmlFor={fieldId}>
      {label && (
        <span className="mb-1.5 block text-[0.78rem] font-medium text-muted-foreground">{label}</span>
      )}
      <textarea ref={ref} id={fieldId} className={cn(FIELD_CLASS, 'resize-y', className)} {...props} />
    </label>
  );
});
