import { Button } from './Button';
import { StatusBadge, type StatusTone } from './StatusBadge';
import { cn } from '../lib/cn';

export type RecoveryState = 'recovery_required' | 'processing' | 'succeeded' | 'expired';

export interface SettlementRecoveryCardProps {
  state: RecoveryState;
  caseId: string;
  amount: string;
  closingPrice: string;
  candidateAlias: string;
  round: number;
  /** The window's end in words: "16:27 UTC". */
  deadline: string;
  /** Time left in words: "1 h 42 m". Shown only while the window is open. */
  timeLeft?: string;
  recoveryUrl?: string;
  onOpenRecovery?: () => void;
  onCopyLink?: () => void;
  onOpenLog?: () => void;
  className?: string;
}

const stateView: Record<RecoveryState, { tone: StatusTone; label: string; title: string; description: string }> = {
  recovery_required: { tone: 'warning', label: 'Action required', title: 'Payment recovery is open', description: 'The candidate has until the deadline to update or confirm their payment method.' },
  processing: { tone: 'neutral', label: 'Processing', title: 'Waiting for the provider', description: 'The attempt is not final yet; the workflow does not move to the next candidate.' },
  succeeded: { tone: 'success', label: 'Paid', title: 'Winner confirmed', description: 'Payment succeeded and the auction has confirmed its winner.' },
  expired: { tone: 'danger', label: 'Expired', title: 'Candidate forfeited', description: 'The window closed without payment; settlement moved to the next candidate.' },
};

/*
  The case's question: will this candidate pay before the window closes? The deadline is the one figure in the hue;
  the facts are a divided strip; the timeline is a list of dots; the guardrails are plain text.
*/
export function SettlementRecoveryCard({ state, caseId, amount, closingPrice, candidateAlias, round, deadline, timeLeft, recoveryUrl, onOpenRecovery, onCopyLink, onOpenLog, className }: SettlementRecoveryCardProps) {
  const view = stateView[state];
  const active = state === 'recovery_required';
  const steps = [
    { title: 'Candidate selected', detail: `Round ${round} · ${amount}`, done: true, now: false },
    { title: 'Payment attempted', detail: 'The provider asked the customer to act', done: true, now: false },
    { title: 'Recovery window', detail: active ? `Open until ${deadline}` : state === 'expired' ? 'Closed without payment' : 'Outcome received', done: state !== 'recovery_required', now: active },
    { title: state === 'expired' ? 'Next candidate' : 'Winner confirmed', detail: state === 'expired' ? 'Settlement amount recalculated' : 'Only after the payment succeeds', done: state === 'succeeded' || state === 'expired', now: state === 'processing' },
  ];

  return (
    <section className={cn('rounded-[8px] border border-line bg-surface', className)} aria-label={`Settlement case ${caseId}`}>
      <header className="flex flex-wrap items-start justify-between gap-x-6 gap-y-3 px-5 pt-4 pb-4">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2"><StatusBadge tone={view.tone}>{view.label}</StatusBadge><span className="font-mono text-xs text-muted">{caseId}</span></div>
          <h2 className="mt-2 text-lg font-semibold text-ink">{view.title}</h2>
          <p className="mt-1 max-w-xl text-sm text-muted">{view.description}</p>
        </div>
        {active && (
          <div className="text-right">
            <p className="text-sm text-muted">Time left</p>
            <p className="text-[26px] leading-8 font-medium text-copper tabular-nums">{timeLeft ?? '—'}</p>
            <p className="text-xs text-muted tabular-nums">until {deadline}</p>
          </div>
        )}
      </header>

      <dl className="grid grid-cols-1 border-y border-rule sm:grid-cols-3 sm:divide-x sm:divide-rule max-sm:divide-y max-sm:divide-rule">
        <Fact label="Settlement amount" value={amount} money />
        <Fact label="Closing price" value={closingPrice} />
        <Fact label="Candidate" value={`${candidateAlias} · round ${round}`} />
      </dl>

      <div className="grid @container gap-6 px-5 py-5 lg:grid-cols-[minmax(0,1fr)_280px]">
        <div>
          <h3 className="text-sm font-medium text-ink">Settlement timeline</h3>
          <ol className="mt-2 divide-y divide-rule">
            {steps.map((step) => (
              <li key={step.title} className="flex min-h-11 items-center gap-3 py-2">
                <span aria-hidden="true" className={cn('size-2 shrink-0 rounded-full', step.done ? 'bg-ink' : step.now ? 'bg-copper' : 'border border-control bg-surface')} />
                <span className="min-w-0 flex-1 text-sm text-ink">{step.title}</span>
                <span className="shrink-0 text-xs text-muted">{step.detail}</span>
              </li>
            ))}
          </ol>
          {active && recoveryUrl && (
            <div className="mt-4">
              <p className="text-sm font-medium text-ink">Recovery link for the candidate</p>
              <div className="mt-2 flex flex-col gap-2 sm:flex-row">
                <input readOnly aria-label="Recovery link" value={recoveryUrl} className="h-9 min-w-0 flex-1 rounded-[6px] border border-control bg-raised px-3 font-mono text-xs text-ink outline-none" />
                <Button variant="secondary" size="lg" onClick={onCopyLink}>Copy link</Button>
                <Button size="lg" onClick={onOpenRecovery}>Open recovery</Button>
              </div>
            </div>
          )}
        </div>
        <aside className="rounded-[6px] bg-raised p-4">
          <h3 className="text-sm font-medium text-ink">Operator guardrails</h3>
          <ul className="mt-2 space-y-1.5 text-sm">
            <Guard ok label="The closing price cannot change" />
            <Guard ok label="The attempt key is idempotent" />
            <Guard ok={state !== 'expired'} label={state === 'expired' ? 'This candidate is no longer eligible' : 'The current candidate is eligible'} />
          </ul>
          <p className="mt-3 text-xs text-muted">An expiry never overwrites a concurrent successful payment before the next candidate is selected.</p>
          <button type="button" onClick={onOpenLog} className="mt-3 text-sm font-medium text-ink underline-offset-[3px] hover:underline">Event log</button>
        </aside>
      </div>
    </section>
  );
}

function Fact({ label, value, money }: { label: string; value: string; money?: boolean }) {
  return <div className="px-5 py-3"><dt className="text-sm text-muted">{label}</dt><dd className={cn('mt-0.5 text-base text-ink tabular-nums', money ? 'font-bold' : 'font-medium')}>{value}</dd></div>;
}

function Guard({ ok, label }: { ok: boolean; label: string }) {
  return <li className="flex items-start gap-2"><span aria-hidden="true" className={cn('mt-1.5 size-2 shrink-0 rounded-full', ok ? 'bg-positive' : 'bg-negative')} /><span className="text-ink">{label}</span></li>;
}
