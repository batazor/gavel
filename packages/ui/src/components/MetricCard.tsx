import type { ReactNode } from 'react';
import { ArrowDownRight, ArrowUpRight } from 'lucide-react';
import { cn } from '../lib/cn';

export interface MetricCardProps {
  label: string;
  value: string;
  hint?: string;
  change?: number;
  icon?: ReactNode;
  className?: string;
}

export function MetricCard({ label, value, hint, change, icon, className }: MetricCardProps) {
  const positive = change !== undefined && change >= 0;
  return (
    <article className={cn('rounded-xl border border-paper-200 bg-paper-50 p-5 shadow-panel', className)}>
      <div className="flex items-start justify-between gap-4">
        <div>
          <p className="m-0 text-xs font-bold uppercase tracking-[0.12em] text-ink-500">{label}</p>
          <p className="mt-3 mb-0 text-3xl font-semibold tracking-[-0.04em] text-ink-950">{value}</p>
        </div>
        {icon && <div className="grid size-10 place-items-center rounded-lg bg-ink-950 text-paper-50">{icon}</div>}
      </div>
      {(hint || change !== undefined) && (
        <div className="mt-5 flex items-center gap-2 border-t border-paper-200 pt-3 text-xs text-ink-500">
          {change !== undefined && (
            <span className={cn('inline-flex items-center gap-0.5 font-bold', positive ? 'text-moss-600' : 'text-red-700')}>
              {positive ? <ArrowUpRight size={14} /> : <ArrowDownRight size={14} />}
              {Math.abs(change)}%
            </span>
          )}
          {hint && <span>{hint}</span>}
        </div>
      )}
    </article>
  );
}
