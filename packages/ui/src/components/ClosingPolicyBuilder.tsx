import { useMemo, useState, type ReactNode } from 'react';
import { AlertTriangle, CalendarClock, Check, Hourglass, LockKeyhole, RotateCcw } from 'lucide-react';
import { cn } from '../lib/cn';

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

function Toggle({ checked, disabled, onChange, label }: { checked: boolean; disabled?: boolean; onChange: (checked: boolean) => void; label: string }) {
  return <button type="button" role="switch" aria-checked={checked} aria-label={label} disabled={disabled} onClick={() => onChange(!checked)} className={cn('relative h-6 w-11 shrink-0 rounded-full transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-copper-500 disabled:cursor-not-allowed disabled:opacity-45', checked ? 'bg-copper-500' : 'bg-ink-500/25')}><span className={cn('absolute top-1 size-4 rounded-full bg-white shadow-sm transition-transform', checked ? 'translate-x-5' : 'translate-x-1')} /></button>;
}

export function ClosingPolicyBuilder({ initialValue, hasBids = false, onChange, className }: ClosingPolicyBuilderProps) {
  const [value, setValue] = useState(initialValue);
  const update = (patch: Partial<ClosingPolicy>) => {
    const next = { ...value, ...patch };
    setValue(next);
    onChange?.(next);
  };
  const inactivityOnly = value.inactivityMinutes !== null && value.scheduledEnd === null && value.hardDeadline === null;
  const valid = value.scheduledEnd !== null || value.inactivityMinutes !== null;
  const summary = useMemo(() => {
    if (!valid) return 'Choose a scheduled end or an inactivity window.';
    const parts = [];
    if (value.scheduledEnd) parts.push(`not before ${value.scheduledEnd.replace('T', ' ')}`);
    if (value.inactivityMinutes) parts.push(`after ${value.inactivityMinutes} min without bids`);
    if (value.hardDeadline) parts.push(`never later than ${value.hardDeadline.replace('T', ' ')}`);
    return `Closes ${parts.join(', ')}.`;
  }, [value, valid]);

  return (
    <section className={cn('overflow-hidden rounded-2xl border border-paper-200 bg-paper-50 shadow-panel', className)}>
      <header className="flex items-start justify-between gap-4 border-b border-paper-200 px-5 py-4"><div><p className="m-0 text-[10px] font-bold uppercase tracking-[0.15em] text-copper-600">Auction rules</p><h2 className="mt-1 mb-0 text-lg font-semibold tracking-[-0.025em]">Closing policy</h2></div>{hasBids && <span className="inline-flex items-center gap-1.5 rounded-full bg-ink-950 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-white"><LockKeyhole size={11} /> Live auction</span>}</header>
      <div className="divide-y divide-paper-200">
        <PolicyRow icon={<CalendarClock size={18} />} title="Scheduled end" description="The auction cannot close before this time." checked={value.scheduledEnd !== null} disabled={hasBids} onToggle={(checked) => update({ scheduledEnd: checked ? '2026-10-24T18:00' : null })}>
          {value.scheduledEnd && <input aria-label="Scheduled end" type="datetime-local" value={value.scheduledEnd} onChange={(event) => update({ scheduledEnd: event.target.value })} className="mt-3 w-full rounded-lg border border-paper-200 bg-white px-3 py-2 text-sm outline-none focus:border-copper-500" />}
        </PolicyRow>
        <PolicyRow icon={<RotateCcw size={18} />} title="Inactivity window" description="Each accepted bid may extend the auction." checked={value.inactivityMinutes !== null} disabled={hasBids} onToggle={(checked) => update({ inactivityMinutes: checked ? 5 : null })}>
          {value.inactivityMinutes !== null && <div className="mt-3 flex items-center gap-2"><input aria-label="Inactivity minutes" type="number" min="1" max="180" value={value.inactivityMinutes} onChange={(event) => update({ inactivityMinutes: Number(event.target.value) })} className="w-24 rounded-lg border border-paper-200 bg-white px-3 py-2 text-sm font-bold tabular-nums outline-none focus:border-copper-500" /><span className="text-xs text-ink-500">minutes after the latest bid</span></div>}
        </PolicyRow>
        <PolicyRow icon={<Hourglass size={18} />} title="Hard deadline" description="A safety ceiling that extensions can never exceed." checked={value.hardDeadline !== null} disabled={hasBids} onToggle={(checked) => update({ hardDeadline: checked ? '2026-10-24T20:00' : null })}>
          {value.hardDeadline && <input aria-label="Hard deadline" type="datetime-local" value={value.hardDeadline} onChange={(event) => update({ hardDeadline: event.target.value })} className="mt-3 w-full rounded-lg border border-paper-200 bg-white px-3 py-2 text-sm outline-none focus:border-copper-500" />}
        </PolicyRow>
      </div>
      <footer className="bg-paper-100 p-5">
        <div className={cn('rounded-xl border p-4', valid ? 'border-moss-500/20 bg-moss-500/8' : 'border-red-700/20 bg-red-700/8')}><div className="flex gap-3">{valid ? <Check className="mt-0.5 shrink-0 text-moss-600" size={17} /> : <AlertTriangle className="mt-0.5 shrink-0 text-red-700" size={17} />}<div><p className="m-0 text-xs font-bold">Effective behavior</p><p className="mt-1 mb-0 text-xs leading-5 text-ink-700">{summary}</p></div></div></div>
        {inactivityOnly && <div role="note" className="mt-3 flex gap-2 text-xs leading-5 text-amber-800"><AlertTriangle className="mt-0.5 shrink-0" size={15} /> Without a hard deadline, bidding can continue indefinitely while new bids arrive.</div>}
        {hasBids && <p className="mt-3 mb-0 text-[11px] text-ink-500">Policy type is locked after the first bid. Operators may still reschedule the scheduled end elsewhere.</p>}
      </footer>
    </section>
  );
}

function PolicyRow({ icon, title, description, checked, disabled, onToggle, children }: { icon: ReactNode; title: string; description: string; checked: boolean; disabled?: boolean; onToggle: (checked: boolean) => void; children?: ReactNode }) {
  return <div className="grid grid-cols-[36px_1fr_auto] gap-3 p-5"><span className="grid size-9 place-items-center rounded-lg bg-paper-100 text-ink-700">{icon}</span><div><h3 className="m-0 text-sm font-semibold">{title}</h3><p className="mt-1 mb-0 text-xs leading-5 text-ink-500">{description}</p>{children}</div><Toggle checked={checked} disabled={disabled} onChange={onToggle} label={`${title}: ${checked ? 'enabled' : 'disabled'}`} /></div>;
}
