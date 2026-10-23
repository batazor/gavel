import type { HTMLAttributes } from 'react';
import { cn } from '../lib/cn';

export type StatusTone = 'neutral' | 'info' | 'success' | 'warning' | 'danger';

export interface StatusBadgeProps extends HTMLAttributes<HTMLSpanElement> {
  tone?: StatusTone;
  /** A 6px dot before the label, for a live or running state. */
  dot?: boolean;
  /** The dot breathes: a ring pulses out of it, so a live state reads as alive. Off under prefers-reduced-motion. */
  pulse?: boolean;
}

/* A pale tint with dark text of the same hue, 22px tall, 12px at 500, sentence case. One per row.
   A dotted badge pulses by default: the dot marks something happening now. */
const tones: Record<StatusTone, string> = {
  neutral: 'bg-pill-neutral text-pill-neutral-ink',
  info: 'bg-pill-neutral text-pill-neutral-ink',
  success: 'bg-pill-positive text-pill-positive-ink',
  warning: 'bg-pill-warning text-pill-warning-ink',
  danger: 'bg-pill-negative text-pill-negative-ink',
};

const dots: Record<StatusTone, string> = {
  neutral: 'bg-faint', info: 'bg-faint', success: 'bg-positive', warning: 'bg-warning', danger: 'bg-negative',
};

export function StatusBadge({ className, tone = 'neutral', dot = false, pulse = dot, children, ...props }: StatusBadgeProps) {
  return (
    <span className={cn('inline-flex h-[22px] w-fit shrink-0 items-center gap-1.5 rounded-full px-2 text-xs font-medium whitespace-nowrap', tones[tone], className)} {...props}>
      {dot && <span aria-hidden="true" className={cn('size-1.5 rounded-full', pulse && 'live-dot', dots[tone])} />}
      {children}
    </span>
  );
}
