import { useMemo, useState, type ReactNode } from 'react';
import { CalendarClock, ChevronRight, CircleDollarSign, Download, Eye, Gavel, MoreHorizontal, PackagePlus, Search, ShieldCheck, Users } from 'lucide-react';
import { Button } from '../components/Button';
import { Panel } from '../components/Panel';
import { StatusBadge, type StatusTone } from '../components/StatusBadge';
import { cn } from '../lib/cn';
import { OpsPageShell, type OpsSection } from './PageShell';

type LotState = 'Live' | 'Draft' | 'Review' | 'Closed' | 'Withdrawn';
type Lot = { id: string; title: string; seller: string; category: string; price: string; bids: number; viewers: number; ends: string; state: LotState };
const lots: Lot[] = [
  { id: 'GV-2841', title: 'Omega Speedmaster, 1969', seller: 'Elena Marlowe', category: 'Watches', price: '€42,800', bids: 184, viewers: 1248, ends: '08:42', state: 'Live' },
  { id: 'GV-2839', title: 'Eames lounge chair', seller: 'Studio Nord', category: 'Design', price: '€8,240', bids: 47, viewers: 386, ends: '02:18', state: 'Live' },
  { id: 'GV-2836', title: 'Leica M6 Titanium', seller: 'Haruto Klein', category: 'Cameras', price: '€11,400', bids: 63, viewers: 719, ends: '1d 04h', state: 'Live' },
  { id: 'GV-2817', title: 'Untitled lithograph, 1974', seller: 'Atelier 17', category: 'Art', price: '€16,900', bids: 0, viewers: 0, ends: '—', state: 'Review' },
  { id: 'GV-2802', title: 'Børge Mogensen cabinet', seller: 'Studio Nord', category: 'Design', price: '€6,800', bids: 0, viewers: 0, ends: '—', state: 'Draft' },
  { id: 'GV-2788', title: 'Cartier Tank Louis, 1982', seller: 'Maison Keller', category: 'Watches', price: '€9,600', bids: 81, viewers: 940, ends: 'Oct 21', state: 'Closed' },
];
const tones: Record<LotState, StatusTone> = { Live: 'success', Draft: 'neutral', Review: 'warning', Closed: 'info', Withdrawn: 'danger' };

export function LotsManagementPage({ onNavigate, onCreateLot, onOpenLot }: { onNavigate?: (section: OpsSection) => void; onCreateLot?: () => void; onOpenLot?: (lotId: string) => void } = {}) {
  const [query, setQuery] = useState('');
  const [state, setState] = useState<'All' | LotState>('All');
  const [selected, setSelected] = useState<Lot>(lots[0]);
  const filtered = useMemo(() => lots.filter((lot) => (state === 'All' || lot.state === state) && `${lot.id} ${lot.title} ${lot.seller}`.toLowerCase().includes(query.toLowerCase())), [query, state]);
  return <OpsPageShell section="lots" onNavigate={onNavigate} title="Lots & auctions" subtitle={`${lots.length.toLocaleString()} records · live catalog projection`} actions={<Button size="sm" onClick={onCreateLot} leadingIcon={<PackagePlus size={15} />}>Create lot</Button>}>
    <div className="grid gap-5 xl:grid-cols-[1fr_300px]">
      <Panel className="min-w-0">
        <div className="flex flex-col gap-3 border-b border-paper-200 p-4 sm:flex-row sm:items-center">
          <label className="flex min-w-0 flex-1 items-center gap-2 rounded-lg border border-paper-200 bg-white px-3 py-2 text-ink-500"><Search size={15} /><input aria-label="Search lots" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Lot, seller or ID" className="min-w-0 flex-1 border-0 bg-transparent text-sm outline-none" /></label>
          <div className="flex gap-2 overflow-x-auto">{(['All', 'Live', 'Review', 'Draft', 'Closed'] as const).map((item) => <button key={item} onClick={() => setState(item)} className={cn('rounded-lg border px-3 py-2 text-xs font-semibold', state === item ? 'border-ink-950 bg-ink-950 text-white' : 'border-paper-200 bg-white text-ink-700 hover:border-ink-500')}>{item}</button>)}</div>
          <Button variant="secondary" size="sm" leadingIcon={<Download size={14} />}>Export</Button>
        </div>
        <div className="overflow-x-auto"><table className="w-full min-w-[600px] border-collapse text-left"><thead><tr className="border-b border-paper-200 bg-paper-100/60 text-[10px] uppercase tracking-[0.12em] text-ink-500"><th className="px-3 py-3"><input aria-label="Select all lots" type="checkbox" /></th><th className="px-3 py-3">Lot</th><th className="hidden px-3 py-3 lg:table-cell">Seller</th><th className="px-3 py-3">Price</th><th className="px-3 py-3">Activity</th><th className="px-3 py-3">Ends</th><th className="px-3 py-3">State</th><th className="px-3 py-3"><span className="sr-only">Actions</span></th></tr></thead><tbody>{filtered.map((lot) => <tr key={lot.id} tabIndex={0} onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') setSelected(lot); }} onClick={() => setSelected(lot)} className={cn('cursor-pointer border-b border-paper-200/70 last:border-0 hover:bg-paper-100/55 focus-visible:outline-2 focus-visible:outline-inset focus-visible:outline-copper-500', selected.id === lot.id && 'bg-copper-500/5 shadow-[inset_3px_0_0_#bd5634]')}><td className="px-3 py-3"><input aria-label={`Select ${lot.id}`} type="checkbox" onClick={(event) => event.stopPropagation()} /></td><td className="px-3 py-3"><div className="flex items-center gap-2"><span className="grid size-9 shrink-0 place-items-center rounded-lg bg-ink-950 text-white/40"><Gavel size={16} /></span><div><div className="max-w-40 text-sm font-semibold leading-5">{lot.title}</div><div className="mt-1 font-mono text-[9px] text-ink-500">{lot.id} · {lot.category}</div></div></div></td><td className="hidden px-3 py-3 text-xs text-ink-700 lg:table-cell">{lot.seller}</td><td className="px-3 py-3 text-sm font-bold tabular-nums">{lot.price}</td><td className="px-3 py-3 text-xs text-ink-500"><strong className="text-ink-950">{lot.bids}</strong> bids<br /><span className="hidden xl:inline">{lot.viewers.toLocaleString()} watching</span></td><td className="px-3 py-3 font-mono text-xs">{lot.ends}</td><td className="px-3 py-3"><StatusBadge tone={tones[lot.state]}>{lot.state}</StatusBadge></td><td className="px-3 py-3"><button aria-label={`Actions for ${lot.id}`} className="rounded p-2 text-ink-500 hover:bg-paper-200"><MoreHorizontal size={16} /></button></td></tr>)}</tbody></table>{filtered.length === 0 && <div className="p-12 text-center text-sm text-ink-500">No lots match these filters.</div>}</div>
        <footer className="flex items-center justify-between border-t border-paper-200 px-4 py-3 text-xs text-ink-500"><span>{filtered.length} results</span><div className="flex gap-1"><button className="rounded border border-paper-200 px-2 py-1">1</button><button className="rounded px-2 py-1">2</button><button className="rounded px-2 py-1">3</button></div></footer>
      </Panel>
      <LotInspector lot={selected} onOpen={onOpenLot ? () => onOpenLot(selected.id) : undefined} />
    </div>
  </OpsPageShell>;
}

function LotInspector({ lot, onOpen }: { lot: Lot; onOpen?: () => void }) {
  return <aside className="h-fit overflow-hidden rounded-xl border border-paper-200 bg-paper-50 shadow-panel xl:sticky xl:top-5"><div className="aspect-[16/10] bg-[radial-gradient(circle_at_55%_35%,#777168,#282725_70%)]"><div className="grid h-full place-items-center text-white/35"><Gavel size={44} strokeWidth={1} /></div></div><div className="p-4"><div className="flex items-start justify-between gap-3"><div><p className="m-0 font-mono text-[9px] text-ink-500">{lot.id}</p><h2 className="mt-1 mb-0 text-base font-semibold leading-5">{lot.title}</h2></div><StatusBadge tone={tones[lot.state]}>{lot.state}</StatusBadge></div><dl className="mt-5 space-y-3 text-xs"><InspectorFact icon={<CircleDollarSign size={14} />} label="Current price" value={lot.price} /><InspectorFact icon={<Users size={14} />} label="Bid activity" value={`${lot.bids} bids · ${lot.viewers} watching`} /><InspectorFact icon={<CalendarClock size={14} />} label="Deadline" value={lot.ends} /><InspectorFact icon={<ShieldCheck size={14} />} label="Seller" value={lot.seller} /></dl><div className="mt-5 grid grid-cols-2 gap-2"><Button variant="secondary" size="sm" leadingIcon={<Eye size={14} />}>Preview</Button><Button size="sm" onClick={onOpen}>Open lot <ChevronRight size={14} /></Button></div><button className="mt-3 flex w-full items-center justify-between rounded-lg px-2 py-2 text-xs text-ink-500 hover:bg-paper-100">View event history <ChevronRight size={14} /></button></div></aside>;
}

function InspectorFact({ icon, label, value }: { icon: ReactNode; label: string; value: string }) {
  return <div className="flex gap-2"><span className="mt-0.5 text-ink-500">{icon}</span><div><dt className="text-ink-500">{label}</dt><dd className="mt-0.5 ml-0 font-semibold text-ink-950">{value}</dd></div></div>;
}
