import type { ReactNode } from 'react';
import { cn } from '../lib/cn';

export interface KpiProps {
  label: string;
  value: string;
  /** A smaller grey suffix after the value: "/min", "ms". */
  unit?: string;
  /** The change as the screen shows it, with its window in words: "+8 vs yesterday 14:32". */
  change?: string;
  /** Whether the change is good news (colour says good or bad, not up or down). Omit for grey. */
  changeGood?: boolean;
  /** A second line under the value: the baseline, the breakdown. */
  hint?: string;
  /** Money is the one figure set bold. */
  money?: boolean;
  /** A lead figure answers the title row's sentence and is set larger than its siblings. */
  lead?: boolean;
  className?: string;
}

/* A KPI is type, not a tile: a grey label over a figure at 400–500, the change in words. No icon, no sparkline. */
export function Kpi({ label, value, unit, change, changeGood, hint, money, lead, className }: KpiProps) {
  return (
    <div className={cn('min-w-0 px-5 py-4', className)}>
      <p className="text-sm text-muted">{label}</p>
      <p className={cn('mt-1 tabular-nums tracking-[-0.01em] text-ink', lead ? 'text-[32px] leading-9' : 'text-[26px] leading-8', money ? 'font-bold' : 'font-medium')}>
        {value}
        {unit && <span className="ml-1 text-sm font-normal text-muted">{unit}</span>}
      </p>
      {(change || hint) && (
        <p className="mt-1 text-xs text-muted">
          {change && <span className={cn('tabular-nums', changeGood === true && 'text-positive', changeGood === false && 'text-negative')}>{change}</span>}
          {change && hint && ' · '}
          {hint}
        </p>
      )}
    </div>
  );
}

/* One container split by 1px dividers into equal columns; on a phone a 2-up grid, an odd last cell spanning. */
export function KpiStrip({ children, className, columns }: { children: ReactNode; className?: string; columns?: number }) {
  return (
    <div
      className={cn('grid grid-cols-2 overflow-hidden rounded-[8px] border border-line bg-surface sm:auto-cols-fr sm:grid-flow-col sm:grid-cols-none', '[&>*:nth-child(2n)]:border-l [&>*:nth-child(2n)]:border-line [&>*:nth-child(n+3)]:border-t [&>*:nth-child(n+3)]:border-line sm:[&>*+*]:border-l sm:[&>*+*]:border-t-0 [&>*:last-child:nth-child(odd)]:col-span-2 sm:[&>*:last-child:nth-child(odd)]:col-span-1', className)}
      style={columns ? { gridTemplateColumns: undefined } : undefined}
    >
      {children}
    </div>
  );
}

export interface MetricCardProps extends Omit<KpiProps, 'className' | 'lead'> {
  className?: string;
}

/** A single KPI in its own bordered card. Prefer `KpiStrip` for a row of peers. */
export function MetricCard({ className, ...kpi }: MetricCardProps) {
  return (
    <article className={cn('rounded-[8px] border border-line bg-surface', className)}>
      <Kpi {...kpi} />
    </article>
  );
}
