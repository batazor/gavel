import type { HTMLAttributes } from 'react';
import { cn } from '../lib/cn';

export type StatusTone = 'neutral' | 'info' | 'success' | 'warning' | 'danger';

export interface StatusBadgeProps extends HTMLAttributes<HTMLSpanElement> {
  tone?: StatusTone;
  dot?: boolean;
}

const tones: Record<StatusTone, string> = {
  neutral: 'border-ink-500/20 bg-ink-500/8 text-ink-700',
  info: 'border-sky-700/20 bg-sky-700/8 text-sky-800',
  success: 'border-moss-500/25 bg-moss-500/10 text-moss-600',
  warning: 'border-amber-700/20 bg-amber-500/12 text-amber-800',
  danger: 'border-red-700/20 bg-red-700/8 text-red-800',
};

const dots: Record<StatusTone, string> = {
  neutral: 'bg-ink-500', info: 'bg-sky-600', success: 'bg-moss-500', warning: 'bg-amber-600', danger: 'bg-red-700',
};

export function StatusBadge({ className, tone = 'neutral', dot = true, children, ...props }: StatusBadgeProps) {
  return (
    <span
      className={cn('inline-flex w-fit items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] font-bold uppercase tracking-[0.1em]', tones[tone], className)}
      {...props}
    >
      {dot && <span aria-hidden="true" className={cn('size-1.5 rounded-full', dots[tone])} />}
      {children}
    </span>
  );
}
