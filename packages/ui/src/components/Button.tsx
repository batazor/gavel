import type { ButtonHTMLAttributes, ReactNode } from 'react';
import { cn } from '../lib/cn';

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger';
export type ButtonSize = 'sm' | 'md' | 'lg';

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  leadingIcon?: ReactNode;
}

const variants: Record<ButtonVariant, string> = {
  primary: 'border-copper-500 bg-copper-500 text-white shadow-sm hover:border-copper-600 hover:bg-copper-600',
  secondary: 'border-paper-200 bg-paper-50 text-ink-950 shadow-sm hover:border-ink-500 hover:bg-white',
  ghost: 'border-transparent bg-transparent text-ink-700 hover:bg-ink-950/5 hover:text-ink-950',
  danger: 'border-red-700 bg-red-700 text-white shadow-sm hover:border-red-800 hover:bg-red-800',
};

const sizes: Record<ButtonSize, string> = {
  sm: 'min-h-8 px-3 text-xs',
  md: 'min-h-10 px-4 text-sm',
  lg: 'min-h-12 px-5 text-sm',
};

export function Button({
  className,
  variant = 'primary',
  size = 'md',
  leadingIcon,
  children,
  type = 'button',
  ...props
}: ButtonProps) {
  return (
    <button
      type={type}
      className={cn(
        'inline-flex items-center justify-center gap-2 rounded-lg border font-semibold tracking-[0.01em] transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-copper-500 disabled:pointer-events-none disabled:opacity-45',
        variants[variant],
        sizes[size],
        className,
      )}
      {...props}
    >
      {leadingIcon && <span aria-hidden="true" className="size-4">{leadingIcon}</span>}
      {children}
    </button>
  );
}
