import type { ReactNode } from 'react';
import { AlertTriangle, Check, ChevronRight, Clock3, Copy, CreditCard, ShieldCheck, UserRound } from 'lucide-react';
import { Button } from './Button';
import { StatusBadge } from './StatusBadge';
import { cn } from '../lib/cn';

export type RecoveryState = 'recovery_required' | 'processing' | 'succeeded' | 'expired';

export interface SettlementRecoveryCardProps {
  state: RecoveryState;
  caseId: string;
  amount: string;
  closingPrice: string;
  candidateAlias: string;
  round: number;
  deadline: string;
  recoveryUrl?: string;
  onOpenRecovery?: () => void;
  onCopyLink?: () => void;
  className?: string;
}

const stateView: Record<RecoveryState, { tone: 'warning' | 'info' | 'success' | 'danger'; label: string; title: string; description: string }> = {
  recovery_required: { tone: 'warning', label: 'Action required', title: 'Payment recovery in progress', description: 'The candidate has two hours to update or confirm their payment method.' },
  processing: { tone: 'info', label: 'Processing', title: 'Waiting for provider outcome', description: 'The payment attempt is non-terminal. The workflow will not advance candidates.' },
  succeeded: { tone: 'success', label: 'Paid', title: 'Winner confirmed', description: 'Payment succeeded and the auction has confirmed its winner.' },
  expired: { tone: 'danger', label: 'Expired', title: 'Candidate forfeited', description: 'The recovery window ended and settlement moved to the next candidate.' },
};

export function SettlementRecoveryCard({ state, caseId, amount, closingPrice, candidateAlias, round, deadline, recoveryUrl, onOpenRecovery, onCopyLink, className }: SettlementRecoveryCardProps) {
  const view = stateView[state];
  const active = state === 'recovery_required';
  return (
    <section className={cn('@container overflow-hidden rounded-2xl border border-paper-200 bg-paper-50 shadow-panel', className)}>
      <header className="flex flex-wrap items-start justify-between gap-4 border-b border-paper-200 px-5 py-5">
        <div><StatusBadge tone={view.tone}>{view.label}</StatusBadge><h2 className="mt-3 mb-1 text-xl font-semibold tracking-[-0.03em]">{view.title}</h2><p className="m-0 max-w-xl text-xs leading-5 text-ink-500">{view.description}</p></div>
        <span className="font-mono text-[10px] text-ink-500">{caseId}</span>
      </header>
      <div className="grid @2xl:grid-cols-[1fr_230px]">
        <div className="p-5">
          <div className="grid gap-3 sm:grid-cols-3">
            <Fact icon={<CreditCard size={15} />} label="Settlement amount" value={amount} strong />
            <Fact icon={<ShieldCheck size={15} />} label="Closing price" value={closingPrice} />
            <Fact icon={<UserRound size={15} />} label="Candidate" value={`${candidateAlias} · round ${round}`} />
          </div>
          <div className="mt-5 rounded-xl border border-paper-200 bg-paper-100 p-4">
            <div className="flex items-center justify-between gap-3"><p className="m-0 text-xs font-bold">Settlement timeline</p>{active && <span className="font-mono text-[10px] text-copper-600">deadline {deadline}</span>}</div>
            <ol className="mt-4 mb-0 list-none p-0">
              <TimelineStep done title="Candidate selected" detail={`Round ${round} · amount ${amount}`} />
              <TimelineStep done title="Payment attempted" detail="Provider requested customer action" />
              <TimelineStep done={state !== 'recovery_required'} active={active} title="Recovery window" detail={active ? `Open until ${deadline}` : state === 'expired' ? 'Expired before payment completed' : 'Payment outcome received'} />
              <TimelineStep done={state === 'succeeded' || state === 'expired'} active={state === 'processing'} last title={state === 'expired' ? 'Next candidate' : 'Confirm winner'} detail={state === 'expired' ? 'Recalculate settlement amount' : 'Only after PaymentSucceeded'} />
            </ol>
          </div>
          {active && recoveryUrl && <div className="mt-4 flex flex-col gap-2 sm:flex-row"><div className="min-w-0 flex-1 rounded-lg border border-paper-200 bg-white px-3 py-2 font-mono text-[10px] leading-5 text-ink-700 break-all">{recoveryUrl}</div><Button variant="secondary" leadingIcon={<Copy size={14} />} onClick={onCopyLink}>Copy link</Button><Button onClick={onOpenRecovery}>Open recovery</Button></div>}
        </div>
        <aside className="border-t border-paper-200 bg-ink-950 p-5 text-white @2xl:border-t-0 @2xl:border-l">
          <p className="m-0 text-[10px] font-bold uppercase tracking-[0.14em] text-white/65">Operator guardrails</p>
          <div className="mt-4 space-y-3 text-xs">
            <Guard ok label="Closing price is immutable" />
            <Guard ok label="Attempt key is idempotent" />
            <Guard ok={state !== 'expired'} label="Current candidate eligible" />
          </div>
          <div className="mt-5 border-t border-white/10 pt-4"><div className="flex items-center gap-2 text-gold-400"><Clock3 size={15} /><span className="text-xs font-bold">{active ? '01:42:18 left' : 'Timer stopped'}</span></div><p className="mt-2 mb-0 text-[10px] leading-4 text-white/65">Expiry never overwrites a concurrent successful payment before the next candidate is selected.</p></div>
          <button className="mt-5 flex w-full items-center justify-between rounded-lg border border-white/15 px-3 py-2.5 text-left text-xs text-white/75 hover:bg-white/8 hover:text-white">Open event log <ChevronRight size={14} /></button>
        </aside>
      </div>
    </section>
  );
}

function Fact({ icon, label, value, strong }: { icon: ReactNode; label: string; value: string; strong?: boolean }) {
  return <div className="rounded-xl border border-paper-200 bg-white p-3"><div className="flex items-center gap-1.5 text-ink-500">{icon}<span className="text-[10px] font-bold uppercase tracking-[0.08em]">{label}</span></div><p className={cn('mt-2 mb-0 text-sm tabular-nums', strong ? 'font-bold text-copper-600' : 'font-semibold')}>{value}</p></div>;
}

function TimelineStep({ done, active, title, detail, last }: { done?: boolean; active?: boolean; title: string; detail: string; last?: boolean }) {
  return <li className="relative grid grid-cols-[24px_1fr] gap-3 pb-4 last:pb-0">{!last && <span className="absolute top-5 bottom-0 left-[11px] w-px bg-paper-200" />}<span className={cn('relative z-10 grid size-6 place-items-center rounded-full border text-[10px]', done ? 'border-moss-500 bg-moss-500 text-white' : active ? 'border-copper-500 bg-copper-500 text-white ring-4 ring-copper-500/10' : 'border-paper-200 bg-paper-50 text-ink-500')}>{done ? <Check size={12} /> : active ? <Clock3 size={12} /> : <span className="size-1.5 rounded-full bg-current" />}</span><div><p className="m-0 text-xs font-semibold">{title}</p><p className="mt-0.5 mb-0 text-[10px] text-ink-500">{detail}</p></div></li>;
}

function Guard({ ok, label }: { ok: boolean; label: string }) {
  return <div className="flex items-start gap-2">{ok ? <Check className="mt-0.5 shrink-0 text-moss-400" size={14} /> : <AlertTriangle className="mt-0.5 shrink-0 text-gold-400" size={14} />}<span className="text-white/75">{label}</span></div>;
}
