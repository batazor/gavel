import type { HTMLAttributes, ReactNode } from 'react';
import { cn } from '../lib/cn';

export interface PanelProps extends HTMLAttributes<HTMLElement> {
  title?: string;
  eyebrow?: string;
  action?: ReactNode;
}

export function Panel({ title, eyebrow, action, children, className, ...props }: PanelProps) {
  return (
    <section className={cn('overflow-hidden rounded-xl border border-paper-200 bg-paper-50 shadow-panel', className)} {...props}>
      {(title || eyebrow || action) && (
        <header className="flex items-center justify-between gap-4 border-b border-paper-200 px-5 py-4">
          <div>
            {eyebrow && <p className="m-0 text-[10px] font-bold uppercase tracking-[0.16em] text-copper-600">{eyebrow}</p>}
            {title && <h2 className="mt-1 mb-0 text-base font-semibold tracking-[-0.02em] text-ink-950">{title}</h2>}
          </div>
          {action}
        </header>
      )}
      {children}
    </section>
  );
}
