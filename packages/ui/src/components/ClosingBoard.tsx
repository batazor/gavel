import { formatClock, formatMoney } from '../lib/format';
import { cn } from '../lib/cn';

export interface ClosingBoardRow {
  id: string;
  title: string;
  price: number;
  /** Scheduled end, minutes since midnight. */
  scheduledEnd: number;
  /** The soft-close extension already granted, minutes since midnight. */
  extendedTo?: number | null;
  onOpen?: () => void;
}

export interface ClosingBoardProps {
  rows: ClosingBoardRow[];
  /** Minutes since midnight. */
  now: number;
  /** The strip's length in minutes. */
  windowMinutes?: number;
  className?: string;
}

/*
  The closing board: the auction house's own element. Each live lot on one strip of the next hour,
  its scheduled hammer as an ink dot, the soft-close extension it has already earned as a copper
  segment after it, and "now" as a 1px copper line. Lots are ordered by when they really close.
*/
export function ClosingBoard({ rows, now, windowMinutes = 60, className }: ClosingBoardProps) {
  const start = now;
  const end = now + windowMinutes;
  const pct = (minutes: number) => `${Math.min(100, Math.max(0, ((minutes - start) / windowMinutes) * 100))}%`;
  const axis = [0, 30, 60].map((m) => start + (m * windowMinutes) / 60);

  return (
    <div className={cn('min-w-0', className)}>
      <div className="grid grid-cols-[minmax(0,1fr)] gap-x-4 sm:grid-cols-[200px_minmax(0,1fr)_96px]">
        <div className="hidden sm:block" />
        <div className="relative hidden h-5 sm:block">
          {axis.map((minutes, index) => (
            <span key={minutes} className={cn('absolute top-0 text-xs text-faint tabular-nums', index === 0 ? 'translate-x-0' : index === axis.length - 1 ? '-translate-x-full' : '-translate-x-1/2')} style={{ left: pct(minutes) }}>
              {formatClock(minutes)}
            </span>
          ))}
        </div>
        <div className="hidden text-right text-xs text-faint sm:block">Closes</div>
      </div>
      <ol className="mt-1 divide-y divide-rule">
        {rows.map((row) => {
          const closes = row.extendedTo ?? row.scheduledEnd;
          const extended = row.extendedTo !== null && row.extendedTo !== undefined && row.extendedTo > row.scheduledEnd;
          return (
            <li key={row.id} className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4 gap-y-2 py-2.5 sm:grid-cols-[200px_minmax(0,1fr)_96px] sm:min-h-12 sm:py-0">
              <button type="button" onClick={row.onOpen} disabled={!row.onOpen} className="min-w-0 py-1 text-left disabled:cursor-default sm:h-11">
                <span className="block truncate text-sm font-medium text-ink">{row.title}</span>
                <span className="block font-mono text-xs text-faint">{row.id}</span>
              </button>
              <div className="relative col-span-2 h-6 sm:col-span-1 sm:h-11" aria-hidden="true">
                <span className="absolute top-1/2 right-0 left-0 h-px bg-rule" />
                <span className="absolute top-0 bottom-0 w-px bg-copper" style={{ left: 0 }} />
                {extended && <span className="absolute top-1/2 h-0.5 -translate-y-1/2 bg-copper" style={{ left: pct(row.scheduledEnd), width: `calc(${pct(row.extendedTo!)} - ${pct(row.scheduledEnd)})` }} />}
                <span className="absolute top-1/2 size-2 -translate-x-1/2 -translate-y-1/2 rounded-full bg-ink" style={{ left: pct(row.scheduledEnd) }} />
                {extended && <span className="absolute top-1/2 h-3 w-0.5 -translate-x-1/2 -translate-y-1/2 bg-copper" style={{ left: pct(row.extendedTo!) }} />}
              </div>
              <div className="text-right sm:py-1">
                <span className="block text-sm text-ink tabular-nums">{formatClock(closes)}</span>
                <span className="block text-xs text-faint tabular-nums">{formatMoney(row.price)}</span>
              </div>
            </li>
          );
        })}
      </ol>
      <p className="mt-3 text-xs text-muted">
        <span className="mr-3 inline-flex items-center gap-1.5"><span aria-hidden="true" className="inline-block size-2 rounded-full bg-ink" />Scheduled hammer</span>
        <span className="mr-3 inline-flex items-center gap-1.5"><span aria-hidden="true" className="inline-block h-0.5 w-3 bg-copper" />Soft-close extension granted</span>
        <span className="inline-flex items-center gap-1.5"><span aria-hidden="true" className="inline-block h-3 w-px bg-copper" />Now {formatClock(now)}</span>
      </p>
    </div>
  );
}
