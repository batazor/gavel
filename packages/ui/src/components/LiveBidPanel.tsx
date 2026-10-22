import { useMemo, useState, type FormEvent } from 'react';
import { ArrowUp, CheckCircle2, Clock3, Eye, ShieldCheck, Sparkles, Wifi } from 'lucide-react';
import { Button } from './Button';
import { StatusBadge } from './StatusBadge';
import { cn } from '../lib/cn';

export interface PublicBid {
  id: string;
  alias: string;
  resultingPrice: number;
  occurredAt: string;
  isMine?: boolean;
}

export interface LiveBidPanelProps {
  currency?: string;
  currentPrice: number;
  minimumBid: number;
  endsAt: string;
  bids: PublicBid[];
  initialMaximum?: number;
  status?: 'leading' | 'outbid' | 'watching';
  onPlaceBid?: (maximum: number) => void;
  className?: string;
}

const money = (value: number, currency: string) => new Intl.NumberFormat('en-GB', {
  style: 'currency', currency, maximumFractionDigits: 0,
}).format(value);

export function LiveBidPanel({
  currency = 'EUR', currentPrice, minimumBid, endsAt, bids, initialMaximum = minimumBid,
  status = 'watching', onPlaceBid, className,
}: LiveBidPanelProps) {
  const [maximum, setMaximum] = useState(initialMaximum);
  const [submitted, setSubmitted] = useState<number | null>(null);
  const statusCopy = {
    leading: { tone: 'success' as const, label: 'You are leading', detail: 'Your maximum stays private' },
    outbid: { tone: 'warning' as const, label: 'You were outbid', detail: 'Raise your private maximum to continue' },
    watching: { tone: 'neutral' as const, label: 'Watching', detail: 'Set a private maximum to join' },
  }[status];
  const quickBids = useMemo(() => [minimumBid, minimumBid + 500, minimumBid + 1_000], [minimumBid]);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (maximum < minimumBid) return;
    setSubmitted(maximum);
    onPlaceBid?.(maximum);
  };

  return (
    <section className={cn('@container overflow-hidden rounded-2xl border border-paper-200 bg-paper-50 shadow-panel', className)}>
      <header className="border-b border-paper-200 bg-ink-950 px-5 py-4 text-paper-50">
        <div className="flex items-center justify-between gap-3">
          <StatusBadge tone="success" style={{ color: '#78c8ad', backgroundColor: 'rgba(85, 167, 142, 0.12)', borderColor: 'rgba(85, 167, 142, 0.35)' }}><Wifi size={11} /> Live</StatusBadge>
          <span className="flex items-center gap-1.5 font-mono text-xs text-paper-200/80"><Clock3 size={13} /> {endsAt}</span>
        </div>
        <div className="mt-5 flex items-end justify-between gap-4">
          <div><p className="m-0 text-[10px] font-bold uppercase tracking-[0.15em] text-paper-200/65">Current price</p><p className="mt-1 mb-0 text-4xl font-semibold tracking-[-0.05em]">{money(currentPrice, currency)}</p></div>
          <div className="text-right text-xs text-paper-200/65"><div>{bids.length} bids</div><div className="mt-1">Step {money(minimumBid - currentPrice, currency)}</div></div>
        </div>
      </header>

      <div className="grid @3xl:grid-cols-[1fr_0.88fr]">
        <div className="border-b border-paper-200 p-4 @3xl:border-r @3xl:border-b-0 @3xl:p-5">
          <div className="flex items-start gap-3 rounded-xl border border-paper-200 bg-paper-100 p-3">
            <span className={cn('mt-0.5 grid size-8 shrink-0 place-items-center rounded-full', status === 'leading' ? 'bg-moss-500 text-white' : status === 'outbid' ? 'bg-amber-500 text-ink-950' : 'bg-ink-950 text-white')}>
              {status === 'leading' ? <CheckCircle2 size={16} /> : status === 'outbid' ? <ArrowUp size={16} /> : <Eye size={16} />}
            </span>
            <div><p className="m-0 text-sm font-bold">{statusCopy.label}</p><p className="mt-1 mb-0 text-xs text-ink-500">{statusCopy.detail}</p></div>
          </div>

          <form onSubmit={submit} className="mt-5">
            <label htmlFor="bid-maximum" className="text-xs font-bold text-ink-700">Your maximum</label>
            <div className="mt-2 flex rounded-xl border border-paper-200 bg-white p-1 focus-within:border-copper-500 focus-within:ring-3 focus-within:ring-copper-500/10">
              <span className="grid w-9 place-items-center text-sm text-ink-500">€</span>
              <input id="bid-maximum" type="number" min={minimumBid} step="100" value={maximum} onChange={(event) => setMaximum(Number(event.target.value))} className="min-w-0 flex-1 border-0 bg-transparent px-1 text-lg font-bold tabular-nums outline-none" />
              <Button type="submit" disabled={maximum < minimumBid}>Place bid</Button>
            </div>
            <p className="mt-2 mb-0 text-[11px] text-ink-500">Minimum {money(minimumBid, currency)}. We bid only enough to keep you ahead.</p>
            <div className="mt-3 flex gap-2">{quickBids.map((value) => <button type="button" key={value} onClick={() => setMaximum(value)} className="flex-1 rounded-lg border border-paper-200 bg-paper-100 px-2 py-2 text-xs font-bold tabular-nums hover:border-ink-500">{money(value, currency)}</button>)}</div>
          </form>

          {submitted !== null && <div role="status" className="mt-4 flex gap-2 rounded-lg border border-moss-500/25 bg-moss-500/10 p-3 text-xs text-moss-600"><ShieldCheck className="shrink-0" size={16} /><span>Maximum {money(submitted, currency)} accepted. Only the resulting public price is visible.</span></div>}
        </div>

        <div className="p-4 @3xl:p-5">
          <div className="flex items-center justify-between"><h2 className="m-0 text-xs font-bold uppercase tracking-[0.13em] text-ink-700">Public bid history</h2><Sparkles size={15} className="text-copper-500" /></div>
          <ol className="mt-4 mb-0 list-none space-y-1 p-0">{bids.slice(0, 6).map((bid, index) => <li key={bid.id} className={cn('flex items-center gap-3 rounded-lg px-2 py-2.5', index === 0 && 'bg-paper-100')}>
            <span className={cn('grid size-7 shrink-0 place-items-center rounded-full text-[10px] font-bold', bid.isMine ? 'bg-copper-500 text-white' : 'bg-paper-200 text-ink-700')}>{bid.alias.slice(0, 2)}</span>
            <div className="min-w-0 flex-1"><p className="m-0 truncate text-xs font-semibold">{bid.isMine ? 'You' : bid.alias}</p><p className="mt-0.5 mb-0 font-mono text-[9px] text-ink-500">{bid.occurredAt}</p></div>
            <span className="text-sm font-bold tabular-nums">{money(bid.resultingPrice, currency)}</span>
          </li>)}</ol>
          <p className="mt-4 mb-0 border-t border-paper-200 pt-3 text-[10px] leading-4 text-ink-500">Aliases are unique to this auction. Private maximums and participant identities are never shown.</p>
        </div>
      </div>
    </section>
  );
}
