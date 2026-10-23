import { useMemo, useState, type ReactNode } from 'react';
import { cn } from '../lib/cn';
import { StatusBadge } from './StatusBadge';

export interface ClosingPolicy {
  scheduledEnd: string | null;
  inactivityMinutes: number | null;
  hardDeadline: string | null;
}

export interface ClosingPolicyBuilderProps {
  initialValue: ClosingPolicy;
  hasBids?: boolean;
  onChange?: (value: ClosingPolicy) => void;
  className?: string;
}

function Switch({ checked, disabled, onChange, label }: { checked: boolean; disabled?: boolean; onChange: (checked: boolean) => void; label: string }) {
  return (
    <button type="button" role="switch" aria-checked={checked} aria-label={label} disabled={disabled} onClick={() => onChange(!checked)} className={cn('relative h-5 w-8 shrink-0 rounded-full transition-colors disabled:cursor-not-allowed disabled:opacity-60', checked ? 'bg-ink' : 'bg-control')}>
      <span className={cn('absolute top-0.5 left-0.5 size-4 rounded-full bg-surface transition-transform', checked && 'translate-x-3')} />
    </button>
  );
}

/* Three rules of the auction's closing, each a row with a switch, and the effective sentence underneath. */
export function ClosingPolicyBuilder({ initialValue, hasBids = false, onChange, className }: ClosingPolicyBuilderProps) {
  const [value, setValue] = useState(initialValue);
  const update = (patch: Partial<ClosingPolicy>) => { const next = { ...value, ...patch }; setValue(next); onChange?.(next); };
  const inactivityOnly = value.inactivityMinutes !== null && value.scheduledEnd === null && value.hardDeadline === null;
  const valid = value.scheduledEnd !== null || value.inactivityMinutes !== null;
  const summary = useMemo(() => {
    if (!valid) return 'Choose a scheduled end or an inactivity window.';
    const parts: string[] = [];
    if (value.scheduledEnd) parts.push(`not before ${value.scheduledEnd.replace('T', ' ')}`);
    if (value.inactivityMinutes) parts.push(`after ${value.inactivityMinutes} minutes without a bid`);
    if (value.hardDeadline) parts.push(`never later than ${value.hardDeadline.replace('T', ' ')}`);
    return `Closes ${parts.join(', ')}.`;
  }, [value, valid]);

  return (
    <section className={cn('panel', className)}>
      <header className="flex min-h-12 items-center justify-between gap-3 px-5 pt-4 pb-3">
        <h2 className="text-base font-medium text-ink">Closing policy</h2>
        {hasBids && <StatusBadge>Locked after the first bid</StatusBadge>}
      </header>
      <div className="divide-y divide-rule border-t border-rule">
        <PolicyRow title="Scheduled end" description="The auction cannot close before this time." checked={value.scheduledEnd !== null} disabled={hasBids} onToggle={(checked) => update({ scheduledEnd: checked ? '2026-10-24T18:00' : null })}>
          {value.scheduledEnd && <input aria-label="Scheduled end" type="datetime-local" value={value.scheduledEnd} onChange={(event) => update({ scheduledEnd: event.target.value })} className="mt-3 h-10 w-full max-w-xs rounded-lg border border-control bg-surface px-3 text-sm outline-none focus:border-ink" />}
        </PolicyRow>
        <PolicyRow title="Inactivity window" description="Each accepted bid may extend the auction." checked={value.inactivityMinutes !== null} disabled={hasBids} onToggle={(checked) => update({ inactivityMinutes: checked ? 5 : null })}>
          {value.inactivityMinutes !== null && <div className="mt-3 flex items-center gap-2"><input aria-label="Inactivity minutes" type="number" min={1} max={180} value={value.inactivityMinutes} onChange={(event) => update({ inactivityMinutes: Number(event.target.value) })} className="h-10 w-24 rounded-lg border border-control bg-surface px-3 text-sm outline-none tabular-nums focus:border-ink" /><span className="text-sm text-muted">minutes after the latest bid</span></div>}
        </PolicyRow>
        <PolicyRow title="Hard deadline" description="A ceiling that no extension can exceed." checked={value.hardDeadline !== null} disabled={hasBids} onToggle={(checked) => update({ hardDeadline: checked ? '2026-10-24T20:00' : null })}>
          {value.hardDeadline && <input aria-label="Hard deadline" type="datetime-local" value={value.hardDeadline} onChange={(event) => update({ hardDeadline: event.target.value })} className="mt-3 h-10 w-full max-w-xs rounded-lg border border-control bg-surface px-3 text-sm outline-none focus:border-ink" />}
        </PolicyRow>
      </div>
      <footer className="border-t border-rule px-5 py-4">
        <p className="flex items-start gap-2 text-sm"><span aria-hidden="true" className={cn('mt-1.5 size-2 shrink-0 rounded-full', valid ? 'bg-positive' : 'bg-negative')} /><span className="text-ink">{summary}</span></p>
        {inactivityOnly && <p role="note" className="mt-2 text-xs text-warning">Without a hard deadline, bidding continues for as long as new bids arrive.</p>}
        {hasBids && <p className="mt-2 text-xs text-muted">The policy type is locked after the first bid. Operators can still move the scheduled end from the lot workspace.</p>}
      </footer>
    </section>
  );
}

function PolicyRow({ title, description, checked, disabled, onToggle, children }: { title: string; description: string; checked: boolean; disabled?: boolean; onToggle: (checked: boolean) => void; children?: ReactNode }) {
  return (
    <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-4 px-5 py-4">
      <div><h3 className="text-sm font-medium text-ink">{title}</h3><p className="mt-0.5 text-sm text-muted">{description}</p>{children}</div>
      <Switch checked={checked} disabled={disabled} onChange={onToggle} label={`${title}: ${checked ? 'on' : 'off'}`} />
    </div>
  );
}
