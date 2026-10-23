import { useMemo, useState } from 'react';
import { MoreHorizontal, Search, X } from 'lucide-react';
import { Button } from '../components/Button';
import { LotCover } from '../components/LotCover';
import { StatusBadge, type StatusTone } from '../components/StatusBadge';
import { cn } from '../lib/cn';
import { formatClock, formatInt, formatMoney } from '../lib/format';
import { lots, today, type Lot, type LotState } from '../data/ops';
import { OpsPageShell, type OpsSection } from './PageShell';

type LotDialogMode = 'preview' | 'history';
type Tab = 'All' | LotState;
const tabs: Tab[] = ['All', 'Live', 'Review', 'Draft', 'Closed'];
const tones: Record<LotState, StatusTone> = { Live: 'success', Draft: 'neutral', Review: 'warning', Closed: 'neutral', Withdrawn: 'danger' };

const closesLabel = (lot: Lot) => {
  if (lot.state === 'Closed') return lot.closesOn ?? '—';
  if (lot.scheduledEnd !== null) return `Today ${formatClock(lot.extendedTo ?? lot.scheduledEnd)}`;
  return lot.closesOn ?? '—';
};

/*
  The screen's question: which lots are live, and which must be reviewed before they can go live?
  Table with tabs and a filter bar: the table sits on the ground with 12px grey headers and no card; the selected
  row drives the inspector rail on the right; the one primary is Create lot.
*/
export function LotsManagementPage({ onNavigate, onCreateLot, onOpenLot }: { onNavigate?: (section: OpsSection) => void; onCreateLot?: () => void; onOpenLot?: (lotId: string) => void } = {}) {
  const [query, setQuery] = useState('');
  const [tab, setTab] = useState<Tab>('All');
  const [selected, setSelected] = useState<Lot>(lots[0]);
  const [actionLot, setActionLot] = useState<Lot | null>(null);
  const [dialog, setDialog] = useState<{ lot: Lot; mode: LotDialogMode } | null>(null);
  const counts = useMemo(() => Object.fromEntries(tabs.map((t) => [t, t === 'All' ? lots.length : lots.filter((lot) => lot.state === t).length])) as Record<Tab, number>, []);
  const filtered = useMemo(() => lots.filter((lot) => (tab === 'All' || lot.state === tab) && `${lot.id} ${lot.title} ${lot.seller}`.toLowerCase().includes(query.toLowerCase())), [query, tab]);
  const live = counts.Live;
  const review = counts.Review;

  return (
    <OpsPageShell
      section="lots"
      onNavigate={onNavigate}
      title="Lots"
      subtitle={`${today.desk} · ${lots.length} lots, ${live} live · ${review === 1 ? 'one lot is' : `${review} lots are`} waiting for review before going live`}
      actions={<><Button variant="secondary">Export</Button><Button onClick={onCreateLot}>Create lot</Button></>}
    >
      <div role="tablist" aria-label="Lot states" className="flex gap-6 overflow-x-auto border-b border-line">
        {tabs.map((item) => (
          <button key={item} role="tab" aria-selected={tab === item} onClick={() => setTab(item)} className={cn('relative shrink-0 py-2.5 text-sm transition-colors', tab === item ? 'text-ink after:absolute after:right-0 after:bottom-[-1px] after:left-0 after:h-0.5 after:bg-ink' : 'text-muted hover:text-ink')}>
            {item} <span className="ml-1 text-xs text-faint tabular-nums">{counts[item]}</span>
          </button>
        ))}
      </div>

      <div className="mt-5 grid items-start gap-5 xl:grid-cols-[minmax(0,1fr)_300px]">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-3">
            <label className="flex h-8 w-full items-center gap-2 rounded-lg border border-control bg-surface px-2.5 text-muted sm:w-[300px]">
              <Search size={14} strokeWidth={1.5} />
              <input aria-label="Search lots" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Lot, seller or ID" className="min-w-0 flex-1 border-0 bg-transparent text-sm text-ink outline-none placeholder:text-faint" />
              {query && <button type="button" aria-label="Clear search" onClick={() => setQuery('')} className="text-muted hover:text-ink"><X size={14} /></button>}
            </label>
            <span className="ml-auto text-sm text-muted tabular-nums">{filtered.length} of {lots.length} lots</span>
          </div>

          <table className="mt-3 w-full text-sm">
            <thead>
              <tr className="text-xs text-muted">
                <th className="h-9 w-[38%] pr-3 text-left font-normal">Lot</th>
                <th className="hidden h-9 px-3 text-left font-normal lg:table-cell">Seller</th>
                <th className="h-9 px-3 text-right font-normal">Price</th>
                <th className="hidden h-9 px-3 text-right font-normal md:table-cell">Bids</th>
                <th className="hidden h-9 px-3 text-right font-normal xl:table-cell">Watching</th>
                <th className="hidden h-9 px-3 text-left font-normal md:table-cell">Closes</th>
                <th className="hidden h-9 px-3 text-left font-normal sm:table-cell">State</th>
                <th className="h-9 w-10"><span className="sr-only">Actions</span></th>
              </tr>
            </thead>
            <tbody className="divide-y divide-rule border-t border-line">
              {filtered.map((lot) => (
                <tr
                  key={lot.id}
                  tabIndex={0}
                  aria-selected={selected.id === lot.id}
                  onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); setSelected(lot); } }}
                  onClick={() => setSelected(lot)}
                  className={cn('h-14 cursor-pointer focus-visible:outline-2 focus-visible:-outline-offset-2', selected.id === lot.id && 'bg-active')}
                >
                  <td className="max-w-0 pr-3">
                    <span className="block truncate font-medium text-ink">{lot.title}</span>
                    <span className="block truncate text-xs text-muted"><span className="sm:hidden">{lot.state} · </span><span className="font-mono">{lot.id}</span> · {lot.category}<span className="md:hidden"> · {formatInt(lot.bids)} bids · {closesLabel(lot)}</span></span>
                  </td>
                  <td className="hidden px-3 whitespace-nowrap text-muted lg:table-cell">{lot.seller}</td>
                  <td className="px-3 text-right font-bold text-ink tabular-nums">{formatMoney(lot.currentPrice)}</td>
                  <td className="hidden px-3 text-right tabular-nums md:table-cell">{lot.bids === 0 ? <span className="text-faint">0</span> : formatInt(lot.bids)}</td>
                  <td className="hidden px-3 text-right text-muted tabular-nums xl:table-cell">{lot.watching === 0 ? <span className="text-faint">0</span> : formatInt(lot.watching)}</td>
                  <td className="hidden px-3 whitespace-nowrap text-muted tabular-nums md:table-cell">{closesLabel(lot)}</td>
                  <td className="hidden px-3 sm:table-cell"><StatusBadge tone={tones[lot.state]} dot={lot.state === 'Live'}>{lot.state}</StatusBadge></td>
                  <td className="w-10 text-right">
                    <button type="button" aria-label={`Actions for ${lot.id}`} aria-haspopup="dialog" onClick={(event) => { event.stopPropagation(); setSelected(lot); setActionLot(lot); }} className="grid size-8 place-items-center rounded-lg text-muted hover:bg-raised hover:text-ink"><MoreHorizontal size={16} /></button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {filtered.length === 0 && <p className="mt-3 grid min-h-20 place-items-center rounded-lg border border-dashed border-control text-sm text-muted">No lots match "{query}" under {tab}.</p>}
        </div>

        <LotInspector lot={selected} onOpen={onOpenLot ? () => onOpenLot(selected.id) : undefined} onPreview={() => setDialog({ lot: selected, mode: 'preview' })} onHistory={() => setDialog({ lot: selected, mode: 'history' })} />
      </div>

      {actionLot && <LotActionsDialog lot={actionLot} onClose={() => setActionLot(null)} onOpen={onOpenLot ? () => onOpenLot(actionLot.id) : undefined} onPreview={() => { setActionLot(null); setDialog({ lot: actionLot, mode: 'preview' }); }} onHistory={() => { setActionLot(null); setDialog({ lot: actionLot, mode: 'history' }); }} />}
      {dialog && <LotDetailDialog lot={dialog.lot} mode={dialog.mode} onClose={() => setDialog(null)} onOpen={onOpenLot ? () => onOpenLot(dialog.lot.id) : undefined} />}
    </OpsPageShell>
  );
}

function LotInspector({ lot, onOpen, onPreview, onHistory }: { lot: Lot; onOpen?: () => void; onPreview: () => void; onHistory: () => void }) {
  return (
    <aside className="grid gap-4 panel p-4 sm:grid-cols-[200px_minmax(0,1fr)] xl:sticky xl:top-6 xl:grid-cols-1">
      <LotCover kind={lot.cover} className="aspect-[16/10] sm:aspect-square xl:aspect-[16/10]" label="No photograph yet" />
      <div className="min-w-0">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="text-base font-medium text-ink">{lot.title}</h2>
          <p className="font-mono text-xs text-muted">{lot.id}</p>
        </div>
        <StatusBadge tone={tones[lot.state]}>{lot.state}</StatusBadge>
      </div>
      <dl className="mt-4 divide-y divide-rule text-sm">
        <Fact label={lot.state === 'Closed' ? 'Hammer price' : lot.bids ? 'Current price' : 'Starting price'} value={formatMoney(lot.currentPrice)} money />
        <Fact label="Bids" value={lot.bids ? `${formatInt(lot.bids)} · ${formatInt(lot.watching)} watching` : '0'} />
        <Fact label="Closes" value={closesLabel(lot)} />
        <Fact label="Seller" value={lot.seller} />
        <Fact label="Category" value={lot.category} />
      </dl>
      <div className="mt-4 grid grid-cols-2 gap-2">
        <Button variant="secondary" onClick={onPreview}>Preview</Button>
        <Button variant="secondary" onClick={onOpen} disabled={!onOpen}>Open lot</Button>
      </div>
      <button type="button" onClick={onHistory} className="mt-3 text-sm font-medium text-ink underline-offset-[3px] hover:underline">Event history</button>
      </div>
    </aside>
  );
}

function Fact({ label, value, money }: { label: string; value: string; money?: boolean }) {
  return <div className="flex h-9 items-center justify-between gap-3"><dt className="text-muted">{label}</dt><dd className={cn('text-right text-ink tabular-nums', money && 'font-bold')}>{value}</dd></div>;
}

function Scrim({ onClose, children, label }: { onClose: () => void; children: React.ReactNode; label: string }) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-ink/30 p-4" onClick={onClose}>
      <section role="dialog" aria-modal="true" aria-label={label} onClick={(event) => event.stopPropagation()} className="max-h-[calc(100dvh-2rem)] w-full max-w-2xl overflow-y-auto rounded-xl border border-line bg-surface shadow-float">
        {children}
      </section>
    </div>
  );
}

function LotActionsDialog({ lot, onClose, onOpen, onPreview, onHistory }: { lot: Lot; onClose: () => void; onOpen?: () => void; onPreview: () => void; onHistory: () => void }) {
  const actions = [
    { label: lot.state === 'Review' ? 'Continue review' : 'Open workspace', description: 'Bidding, details and auction state.', onClick: onOpen },
    { label: 'Preview the public listing', description: 'What bidders see, without leaving the back office.', onClick: onPreview },
    { label: 'Event history', description: 'Domain and operator events for this lot.', onClick: onHistory },
  ];
  return (
    <Scrim onClose={onClose} label={`Actions for ${lot.id}`}>
      <div className="max-w-sm">
        <header className="flex items-start justify-between gap-4 border-b border-line px-5 py-4">
          <div><h2 className="text-base font-medium text-ink">{lot.title}</h2><p className="font-mono text-xs text-muted">{lot.id}</p></div>
          <button type="button" aria-label="Close" onClick={onClose} className="grid size-8 place-items-center rounded-lg text-muted hover:bg-raised hover:text-ink"><X size={16} /></button>
        </header>
        <ul className="py-1">
          {actions.map((action) => (
            <li key={action.label}>
              <button type="button" onClick={action.onClick} disabled={!action.onClick} className="block w-full px-5 py-2.5 text-left hover:bg-raised disabled:opacity-60">
                <span className="block text-sm text-ink">{action.label}</span>
                <span className="block text-xs text-muted">{action.description}</span>
              </button>
            </li>
          ))}
        </ul>
      </div>
    </Scrim>
  );
}

function LotDetailDialog({ lot, mode, onClose, onOpen }: { lot: Lot; mode: LotDialogMode; onClose: () => void; onOpen?: () => void }) {
  const events = [
    { time: '14:31:52', title: lot.bids ? 'BidAccepted' : 'LotSubmittedForReview', detail: lot.bids ? `${formatMoney(lot.currentPrice)} · bidder alias Silver Fox` : 'All required catalogue fields validated' },
    { time: '14:29:44', title: lot.state === 'Live' ? 'ClosingTimeExtended' : 'MediaProcessingCompleted', detail: lot.state === 'Live' ? 'Soft close extended by 2 minutes' : '3 assets ready for publication' },
    { time: '13:58:10', title: 'LotDetailsUpdated', detail: `Operator updated ${lot.category.toLowerCase()} catalogue data` },
  ];
  return (
    <Scrim onClose={onClose} label={mode === 'preview' ? `Public listing of ${lot.id}` : `Event history of ${lot.id}`}>
      <header className="flex items-start justify-between gap-4 border-b border-line px-5 py-4">
        <div>
          <p className="text-xs text-muted">{mode === 'preview' ? 'Public listing' : 'Event history'}</p>
          <h2 className="text-lg font-semibold text-ink">{lot.title}</h2>
          <p className="text-xs text-muted"><span className="font-mono">{lot.id}</span> · {lot.state}</p>
        </div>
        <button type="button" aria-label="Close" onClick={onClose} className="grid size-8 place-items-center rounded-lg text-muted hover:bg-raised hover:text-ink"><X size={16} /></button>
      </header>
      {mode === 'preview' ? (
        <div className="p-5">
          <LotCover kind={lot.cover} className="aspect-[16/7]" iconSize={40} />
          <div className="mt-5 flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
            <div>
              <p className="text-xs text-muted">{lot.category} · {lot.seller}</p>
              <h3 className="mt-1 text-xl font-semibold text-ink">{lot.title}</h3>
              <p className="mt-2 max-w-lg text-sm text-muted">Images, description, price and auction state appear here exactly as bidders will see them.</p>
            </div>
            <div className="shrink-0 rounded-lg bg-raised px-4 py-3 text-right">
              <p className="text-xs text-muted">{lot.bids ? 'Current price' : 'Starting price'}</p>
              <p className="text-2xl font-bold text-copper tabular-nums">{formatMoney(lot.currentPrice)}</p>
              <p className="text-xs text-muted tabular-nums">{formatInt(lot.bids)} bids · {closesLabel(lot)}</p>
            </div>
          </div>
        </div>
      ) : (
        <ol className="divide-y divide-rule px-5">
          {events.map((event) => (
            <li key={`${event.time}-${event.title}`} className="grid grid-cols-[72px_minmax(0,1fr)] gap-3 py-3">
              <span className="text-xs text-muted tabular-nums">{event.time}</span>
              <div><p className="text-sm text-ink">{event.title}</p><p className="text-xs text-muted">{event.detail}</p></div>
            </li>
          ))}
        </ol>
      )}
      <footer className="flex justify-end gap-2 border-t border-line px-5 py-3">
        <Button variant="secondary" onClick={onClose}>Close</Button>
        <Button onClick={onOpen} disabled={!onOpen}>Open workspace</Button>
      </footer>
    </Scrim>
  );
}
