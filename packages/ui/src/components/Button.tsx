import type { ButtonHTMLAttributes, ReactNode } from 'react';
import { cn } from '../lib/cn';

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger';
export type ButtonSize = 'sm' | 'md' | 'lg';

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  leadingIcon?: ReactNode;
}

/*
  Controls: one filled copper primary per screen, a paper secondary with a warm line,
  a text ghost, and a danger that stays outlined until pressed. Radius 8, heights 28/32/36.
*/
const variants: Record<ButtonVariant, string> = {
  primary: 'border-transparent bg-primary text-primary-ink hover:bg-copper-deep disabled:bg-active disabled:text-muted',
  secondary: 'border-control bg-surface text-ink hover:border-copper/50 hover:bg-raised disabled:border-line disabled:text-muted',
  ghost: 'border-transparent bg-transparent text-muted hover:bg-raised hover:text-ink disabled:text-faint',
  danger: 'border-negative/40 bg-surface text-negative hover:bg-pill-negative disabled:border-line disabled:text-muted',
};

const sizes: Record<ButtonSize, string> = {
  sm: 'h-7 px-2.5 text-[13px]',
  md: 'h-8 px-3 text-sm',
  lg: 'h-9 px-4 text-sm',
};

export function Button({ className, variant = 'primary', size = 'md', leadingIcon, children, type = 'button', ...props }: ButtonProps) {
  return (
    <button
      type={type}
      className={cn(
        'lift inline-flex shrink-0 items-center justify-center gap-2 rounded-lg border font-semibold whitespace-nowrap disabled:cursor-not-allowed disabled:transform-none',
        variants[variant],
        sizes[size],
        className,
      )}
      {...props}
    >
      {leadingIcon && <span aria-hidden="true" className="-ml-0.5 grid size-4 place-items-center [&>svg]:size-4">{leadingIcon}</span>}
      {children}
    </button>
  );
}
