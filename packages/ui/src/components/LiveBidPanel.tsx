import { useMemo, useState, type FormEvent } from 'react';
import { Button } from './Button';
import { StatusBadge } from './StatusBadge';
import { cn } from '../lib/cn';
import { formatInt, formatMoney } from '../lib/format';
import type { PublicBid } from '../data/ops';

export type { PublicBid };

export interface LiveBidPanelProps {
  currency?: string;
  /** How this auction closes, in the bidder's words: ["A bid in the last 5 minutes extends the auction by 2 minutes", …]. */
  closingRules?: string[];
  currentPrice: number;
  minimumBid: number;
  endsAt: string;
  bids: PublicBid[];
  /** The total accepted bids when the list is a window of them. */
  totalBids?: number;
  initialMaximum?: number;
  status?: 'leading' | 'outbid' | 'watching';
  onPlaceBid?: (maximum: number) => void | Promise<void>;
  className?: string;
}

const aliasHue = (alias: string) => [...alias].reduce((sum, ch) => sum + ch.charCodeAt(0), 0) % 360;

/*
  The bidder's panel: the one question is "am I leading, and what does the next bid cost?".
  The current price is the main data element and carries the one hue; the rest is ink on paper.
*/
export function LiveBidPanel({ currency = 'EUR', closingRules = ['A bid in the last 5 minutes extends the auction by 2 minutes.', 'No extension past the hard deadline of 15:00 UTC.', 'The winner pays within 2 hours or the lot passes to the next bidder.'], currentPrice, minimumBid, endsAt, bids, totalBids, initialMaximum = minimumBid, status = 'watching', onPlaceBid, className }: LiveBidPanelProps) {
  const [maximum, setMaximum] = useState(initialMaximum);
  const [submitted, setSubmitted] = useState<number | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [submissionError, setSubmissionError] = useState<string | null>(null);
  const money = (value: number) => formatMoney(value, currency);
  const statusCopy = {
    leading: { tone: 'success' as const, label: 'You are leading', detail: 'Your maximum stays private; we bid only enough to keep you ahead.' },
    outbid: { tone: 'warning' as const, label: 'You were outbid', detail: 'Raise your maximum to stay in the auction.' },
    watching: { tone: 'neutral' as const, label: 'Watching', detail: 'Set a private maximum to join the bidding.' },
  }[status];
  const quickBids = useMemo(() => [minimumBid, minimumBid + 500, minimumBid + 1_000], [minimumBid]);
  const total = totalBids ?? bids.length;

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (maximum < minimumBid || submitting) return;
    setSubmitting(true);
    setSubmissionError(null);
    try {
      await onPlaceBid?.(maximum);
      setSubmitted(maximum);
    } catch (error) {
      setSubmitted(null);
      setSubmissionError(error instanceof Error ? error.message : 'The bid could not be placed.');
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <section className={cn('@container rounded-[8px] border border-line bg-surface', className)} aria-label="Live bidding">
      <header className="flex flex-wrap items-start justify-between gap-x-6 gap-y-3 px-5 pt-4 pb-4">
        <div>
          <div className="flex items-center gap-2 text-sm text-muted"><StatusBadge tone="success" dot>Live</StatusBadge><span>Current price</span></div>
          <p className="mt-1 text-[32px] leading-9 font-bold text-copper tabular-nums">{money(currentPrice)}</p>
          <p className="mt-1 text-sm text-muted tabular-nums">{formatInt(total)} bids · next bid {money(minimumBid)}</p>
        </div>
        <div className="text-right">
          <p className="text-sm text-muted">Ends in</p>
          <p className="text-lg font-medium text-ink tabular-nums">{endsAt}</p>
        </div>
      </header>

      <div className="grid border-t border-rule @3xl:grid-cols-[minmax(0,1fr)_minmax(0,0.9fr)]">
        <div className="border-b border-rule p-5 @3xl:border-r @3xl:border-b-0">
          <p className="flex items-start gap-2 text-sm">
            <span aria-hidden="true" className={cn('mt-1.5 size-2 shrink-0 rounded-full', status === 'leading' ? 'bg-positive' : status === 'outbid' ? 'bg-warning' : 'bg-control')} />
            <span><span className="font-medium text-ink">{statusCopy.label}.</span> <span className="text-muted">{statusCopy.detail}</span></span>
          </p>

          <form onSubmit={submit} className="mt-4">
            <label htmlFor="bid-maximum" className="text-sm font-medium text-ink">Your maximum, €</label>
            <div className="mt-2 flex gap-2">
              <input id="bid-maximum" type="number" min={minimumBid} step={100} value={maximum} onChange={(event) => setMaximum(Number(event.target.value))} className="h-10 min-w-0 flex-1 rounded-[6px] border border-control bg-surface px-3 text-base text-ink outline-none tabular-nums focus:border-ink" />
              <Button type="submit" size="lg" disabled={maximum < minimumBid || submitting} className="h-10">{submitting ? 'Placing…' : 'Place bid'}</Button>
            </div>
            <p className="mt-2 text-xs text-muted">Minimum {money(minimumBid)}. Only the resulting public price is shown to others.</p>
            <div className="mt-3 flex gap-2">
              {quickBids.map((value) => <button type="button" key={value} onClick={() => setMaximum(value)} aria-pressed={maximum === value} className={cn('h-8 flex-1 rounded-[6px] border text-sm tabular-nums', maximum === value ? 'border-ink bg-active text-ink' : 'border-control bg-surface text-ink hover:bg-raised')}>{money(value)}</button>)}
            </div>
          </form>

          {submitted !== null && <p role="status" className="mt-4 flex items-start gap-2 text-sm"><span aria-hidden="true" className="mt-1.5 size-2 shrink-0 rounded-full bg-positive" /><span className="text-ink">Maximum {money(submitted)} accepted.</span></p>}
          {submissionError && <p role="alert" className="mt-4 flex items-start gap-2 text-sm"><span aria-hidden="true" className="mt-1.5 size-2 shrink-0 rounded-full bg-negative" /><span className="text-ink">{submissionError}</span></p>}
          {closingRules.length > 0 && (
            <div className="mt-5 border-t border-rule pt-4">
              <h3 className="text-sm font-medium text-ink">How this auction closes</h3>
              <ul className="mt-1 space-y-1 text-sm text-muted">{closingRules.map((rule) => <li key={rule}>{rule}</li>)}</ul>
            </div>
          )}
        </div>

        <div className="p-5 pt-4">
          <div className="flex items-baseline justify-between"><h3 className="text-sm font-medium text-ink">Public bids</h3><span className="text-xs text-faint tabular-nums">latest {Math.min(bids.length, 6)} of {formatInt(total)}</span></div>
          <ol className="mt-2 divide-y divide-rule">
            {bids.slice(0, 6).map((bid) => (
              <li key={bid.id} className="flex h-10 items-center gap-3">
                <span aria-hidden="true" className="grid size-6 shrink-0 place-items-center rounded-full text-[11px] font-medium" style={bid.isMine ? { background: 'var(--color-active)', color: 'var(--color-ink)' } : { background: `hsl(${aliasHue(bid.alias)} 40% 92%)`, color: `hsl(${aliasHue(bid.alias)} 40% 28%)` }}>{bid.alias.slice(0, 2)}</span>
                <span className="min-w-0 flex-1 truncate text-sm text-ink">{bid.isMine ? 'You' : bid.alias}<span className="ml-2 text-xs text-muted tabular-nums">{bid.occurredAt}</span></span>
                <span className={cn('text-sm tabular-nums', bid.isMine ? 'font-medium text-ink' : 'text-ink')}>{money(bid.resultingPrice)}</span>
              </li>
            ))}
          </ol>
          <p className="mt-3 text-xs text-muted">Aliases are unique to this auction. Private maximums and identities are never shown.</p>
        </div>
      </div>
    </section>
  );
}
