import type { HTMLAttributes, ReactNode } from 'react';
import { cn } from '../lib/cn';

export interface PanelProps extends HTMLAttributes<HTMLElement> {
  title?: string;
  /** One grey line beside the title: the period, the scope or the count ("Last 60 minutes", "7 open"). */
  meta?: ReactNode;
  /** @deprecated use `meta`; kept so older call sites compile. Rendered as meta, never as a label above the title. */
  eyebrow?: string;
  action?: ReactNode;
  /** Padding for free-form content. Lists and tables that draw their own rows pass `false`. */
  padded?: boolean;
  headingLevel?: 2 | 3;
}

/*
  A panel is a near-white sheet with a 1px line and an 8px radius, no shadow.
  The header is the title (16/500) with the meta in grey beside it and one action on the right.
*/
export function Panel({ title, meta, eyebrow, action, padded = false, headingLevel = 2, children, className, ...props }: PanelProps) {
  const Heading = headingLevel === 2 ? 'h2' : 'h3';
  const metaNode = meta ?? eyebrow;
  return (
    <section className={cn('min-w-0 rounded-[8px] border border-line bg-surface', className)} {...props}>
      {(title || metaNode || action) && (
        <header className="flex min-h-12 items-center justify-between gap-3 px-5 pt-4 pb-3">
          <div className="flex min-w-0 flex-wrap items-baseline gap-x-3 gap-y-0.5">
            {title && <Heading className="text-base font-medium text-ink">{title}</Heading>}
            {metaNode && <span className="text-xs text-faint">{metaNode}</span>}
          </div>
          {action && <div className="flex shrink-0 items-center gap-2">{action}</div>}
        </header>
      )}
      <div className={cn(padded && 'px-5 pb-5', padded && !(title || metaNode || action) && 'pt-5')}>{children}</div>
    </section>
  );
}

/** One sentence that spans a block when it has nothing to show. The frame and the header stay. */
export function EmptyState({ children, className }: { children: ReactNode; className?: string }) {
  return <p className={cn('mx-5 mb-5 grid min-h-20 place-items-center rounded-[6px] border border-dashed border-control px-4 text-center text-sm text-muted', className)}>{children}</p>;
}

/** The final layout with each value as a flat grey bar at its real size. No shimmer. */
export function Skeleton({ width = '100%', height = 14, className }: { width?: number | string; height?: number; className?: string }) {
  return <span aria-hidden="true" className={cn('block rounded-[4px] bg-raised', className)} style={{ width, height }} />;
}

/** Inside the block that failed: what failed, when the data was last good, and a retry. */
export function ErrorState({ message, lastGood, onRetry }: { message: string; lastGood?: string; onRetry?: () => void }) {
  return (
    <div role="alert" className="mx-5 mb-5 flex flex-wrap items-center gap-x-3 gap-y-1 rounded-[6px] border border-line bg-raised px-4 py-3 text-sm">
      <span aria-hidden="true" className="size-2 rounded-full bg-negative" />
      <span className="text-ink">{message}</span>
      {lastGood && <span className="text-muted">Last updated {lastGood}.</span>}
      {onRetry && <button type="button" onClick={onRetry} className="ml-auto font-medium text-ink underline-offset-[3px] hover:underline">Retry</button>}
    </div>
  );
}
