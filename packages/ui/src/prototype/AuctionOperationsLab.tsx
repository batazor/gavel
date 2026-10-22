// PROTOTYPE: Three admin UI directions, switchable via ?variant=A|B|C in one Storybook canvas.
import { useEffect, useState } from 'react';
import {
  Activity,
  AlertTriangle,
  ArrowLeft,
  ArrowRight,
  Boxes,
  CheckCircle2,
  ChevronRight,
  CircleDollarSign,
  Clock3,
  Gauge,
  Gavel,
  LayoutDashboard,
  ListFilter,
  Radio,
  Search,
  ShieldCheck,
  Users,
  Workflow,
} from 'lucide-react';
import { AdminShell } from '../components/AdminShell';
import { AuctionFlow } from '../components/AuctionFlow';
import { Button } from '../components/Button';
import { MetricCard } from '../components/MetricCard';
import { Panel } from '../components/Panel';
import { StatusBadge } from '../components/StatusBadge';
import { cn } from '../lib/cn';

type Variant = 'A' | 'B' | 'C';
const variants: Variant[] = ['A', 'B', 'C'];
const names: Record<Variant, string> = {
  A: 'Command center',
  B: 'Operations inbox',
  C: 'Workflow canvas',
};

const lots = [
  { id: 'GV-2841', lot: 'Omega Speedmaster, 1969', price: '€42,800', bids: 184, state: 'Live', tone: 'success' as const },
  { id: 'GV-2839', lot: 'Eames lounge chair', price: '€8,240', bids: 47, state: 'Ending', tone: 'warning' as const },
  { id: 'GV-2817', lot: 'Untitled lithograph, 1974', price: '€16,900', bids: 92, state: 'Review', tone: 'info' as const },
  { id: 'GV-2802', lot: 'Leica M6 Titanium', price: '€11,400', bids: 63, state: 'Settled', tone: 'neutral' as const },
];

function VariantA() {
  return (
    <AdminShell
      title="Auction operations"
      subtitle="Live system pulse · 14:32:08 UTC"
      nav={[
        { label: 'Overview', icon: <LayoutDashboard size={16} />, active: true },
        { label: 'Auctions', icon: <Gavel size={16} />, badge: '12' },
        { label: 'Settlements', icon: <CircleDollarSign size={16} /> },
        { label: 'Participants', icon: <Users size={16} /> },
        { label: 'Workflows', icon: <Workflow size={16} /> },
      ]}
      actions={<Button size="sm" leadingIcon={<Gavel size={15} />}>New auction</Button>}
    >
      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        <MetricCard label="Live auctions" value="128" change={8.4} hint="vs yesterday" icon={<Radio size={18} />} />
        <MetricCard label="Bids / minute" value="2,418" change={12.1} hint="7-day peak" icon={<Activity size={18} />} />
        <MetricCard label="Hammer volume" value="€1.84m" change={-2.6} hint="today" icon={<CircleDollarSign size={18} />} />
        <MetricCard label="Needs review" value="7" hint="3 high priority" icon={<AlertTriangle size={18} />} />
      </div>
      <Panel title="Live lots" eyebrow="Market pulse" action={<Button variant="ghost" size="sm">View all <ChevronRight size={14} /></Button>} className="mt-5">
        <div className="overflow-x-auto">
          <table className="w-full min-w-[640px] border-collapse text-left">
            <thead><tr className="border-b border-paper-200 text-[10px] uppercase tracking-[0.13em] text-ink-500">
              <th className="px-5 py-3 font-bold">Lot</th><th className="px-4 py-3 font-bold">Current price</th><th className="px-4 py-3 font-bold">Bids</th><th className="px-4 py-3 font-bold">State</th><th className="px-5 py-3"><span className="sr-only">Actions</span></th>
            </tr></thead>
            <tbody>{lots.map((lot) => <tr key={lot.id} className="border-b border-paper-200/70 last:border-0 hover:bg-white">
              <td className="px-5 py-4"><div className="text-sm font-semibold text-ink-950">{lot.lot}</div><div className="mt-1 font-mono text-[10px] text-ink-500">{lot.id}</div></td>
              <td className="px-4 py-4 text-sm font-bold tabular-nums">{lot.price}</td>
              <td className="px-4 py-4 text-sm text-ink-700">{lot.bids}</td>
              <td className="px-4 py-4"><StatusBadge tone={lot.tone}>{lot.state}</StatusBadge></td>
              <td className="px-5 py-4 text-right"><button aria-label={`Открыть ${lot.id}`} className="rounded p-2 text-ink-500 hover:bg-paper-100 hover:text-ink-950"><ChevronRight size={16} /></button></td>
            </tr>)}</tbody>
          </table>
        </div>
      </Panel>
    </AdminShell>
  );
}

function VariantB() {
  const [selected, setSelected] = useState(0);
  const cases = [
    { title: 'Payment requires recovery', id: 'STL-4912', meta: '€42,800 · 4m ago', tone: 'danger' as const },
    { title: 'Identity review requested', id: 'USR-0834', meta: 'High-value bidder · 11m ago', tone: 'warning' as const },
    { title: 'Auction extension conflict', id: 'GV-2839', meta: 'Soft close · 16m ago', tone: 'warning' as const },
    { title: 'Seller document approved', id: 'DOC-1291', meta: 'Resolved · 29m ago', tone: 'success' as const },
  ];
  return (
    <div className="min-h-[760px] overflow-hidden rounded-2xl border border-ink-950/10 bg-paper-50 shadow-panel">
      <header className="flex min-h-16 items-center gap-5 border-b border-paper-200 px-5">
        <div className="flex items-center gap-2 font-bold tracking-[0.1em]"><Gavel className="text-copper-500" size={19} /> GAVEL</div>
        <div className="h-5 w-px bg-paper-200" />
        <h1 className="m-0 text-sm font-semibold">Operations inbox</h1>
        <div className="ml-auto flex items-center gap-2"><StatusBadge tone="success">Systems nominal</StatusBadge><Button size="sm" variant="secondary">Elena V.</Button></div>
      </header>
      <div className="grid min-h-[694px] md:grid-cols-[320px_1fr]">
        <aside className="border-r border-paper-200 bg-paper-100/60">
          <div className="p-4">
            <label className="flex items-center gap-2 rounded-lg border border-paper-200 bg-white px-3 py-2 text-sm text-ink-500"><Search size={15} /><input aria-label="Поиск кейсов" placeholder="Search cases" className="min-w-0 flex-1 border-0 bg-transparent outline-none placeholder:text-ink-500" /></label>
            <div className="mt-3 flex items-center justify-between"><span className="text-[10px] font-bold uppercase tracking-[0.15em] text-ink-500">Priority queue · 7</span><button aria-label="Фильтры" className="rounded p-1.5 hover:bg-paper-200"><ListFilter size={15} /></button></div>
          </div>
          <div>{cases.map((item, index) => <button key={item.id} onClick={() => setSelected(index)} className={cn('w-full border-t border-paper-200 px-4 py-4 text-left transition-colors', selected === index ? 'bg-paper-50 shadow-[inset_3px_0_0_#bd5634]' : 'hover:bg-paper-50/70')}>
            <div className="flex items-start gap-3"><span className={cn('mt-1.5 size-2 shrink-0 rounded-full', item.tone === 'danger' ? 'bg-red-700' : item.tone === 'warning' ? 'bg-amber-600' : 'bg-moss-500')} /><div><div className="text-sm font-semibold leading-5">{item.title}</div><div className="mt-1 font-mono text-[10px] text-ink-500">{item.id} · {item.meta}</div></div></div>
          </button>)}</div>
        </aside>
        <main className="p-6 lg:p-9">
          <div className="mx-auto max-w-3xl">
            <div className="flex flex-wrap items-start justify-between gap-4">
              <div><StatusBadge tone="danger">Action required</StatusBadge><h2 className="mt-4 mb-1 text-2xl font-semibold tracking-[-0.035em]">{cases[selected].title}</h2><p className="m-0 font-mono text-xs text-ink-500">{cases[selected].id} · settlement workflow</p></div>
              <div className="flex gap-2"><Button variant="secondary">Assign</Button><Button>Open recovery</Button></div>
            </div>
            <div className="mt-8 grid gap-5 lg:grid-cols-[1fr_240px]">
              <Panel title="What happened" eyebrow="Timeline">
                <ol className="m-0 list-none p-5">
                  {[
                    ['14:28:09', 'Payment authorization declined', 'Provider returned insufficient_funds. No retry was issued.'],
                    ['14:27:44', 'Settlement started', 'Candidate #1 entered the two-hour recovery window.'],
                    ['14:27:41', 'Auction closed', 'Winning bid €42,800. Stream version 18,402.'],
                  ].map(([time, title, detail], index) => <li key={time} className="relative grid grid-cols-[72px_1fr] gap-4 pb-6 last:pb-0">
                    {index < 2 && <span className="absolute top-4 bottom-0 left-[83px] w-px bg-paper-200" />}
                    <time className="font-mono text-[10px] text-ink-500">{time}</time><div className="relative"><span className={cn('absolute top-1 -left-[21px] size-2 rounded-full ring-4 ring-paper-50', index === 0 ? 'bg-red-700' : 'bg-ink-500')} /><p className="m-0 text-sm font-semibold">{title}</p><p className="mt-1 mb-0 text-xs leading-5 text-ink-500">{detail}</p></div>
                  </li>
                  )}
                </ol>
              </Panel>
              <div className="space-y-5">
                <Panel title="Guardrails"><dl className="m-0 space-y-3 p-4 text-xs"><div className="flex justify-between"><dt className="text-ink-500">Window left</dt><dd className="m-0 font-mono font-bold">01:42:18</dd></div><div className="flex justify-between"><dt className="text-ink-500">Candidate</dt><dd className="m-0 font-bold">1 of 3</dd></div><div className="flex justify-between"><dt className="text-ink-500">Auto retry</dt><dd className="m-0 font-bold">Disabled</dd></div></dl></Panel>
                <Panel title="Checks"><div className="space-y-3 p-4 text-xs"><p className="m-0 flex items-center gap-2 text-moss-600"><CheckCircle2 size={15} /> Closing price immutable</p><p className="m-0 flex items-center gap-2 text-moss-600"><ShieldCheck size={15} /> Actor authorized</p><p className="m-0 flex items-center gap-2 text-amber-800"><Clock3 size={15} /> Recovery timer active</p></div></Panel>
              </div>
            </div>
          </div>
        </main>
      </div>
    </div>
  );
}

function VariantC() {
  return (
    <div className="min-h-[760px] overflow-hidden rounded-2xl border border-ink-950/10 bg-ink-950 text-paper-50 shadow-panel">
      <header className="flex min-h-16 items-center border-b border-white/10 px-5">
        <div className="flex items-center gap-2 font-bold tracking-[0.1em]"><Gavel className="text-copper-400" size={19} /> GAVEL</div>
        <nav className="ml-8 hidden items-center gap-1 md:flex"><button className="rounded-md bg-white/10 px-3 py-1.5 text-xs font-semibold">Live topology</button><button className="rounded-md px-3 py-1.5 text-xs text-white/50 hover:text-white">Runs</button><button className="rounded-md px-3 py-1.5 text-xs text-white/50 hover:text-white">Definitions</button></nav>
        <div className="ml-auto flex gap-2"><Button variant="ghost" size="sm" className="text-paper-200 hover:bg-white/10 hover:text-white">Pause updates</Button><Button size="sm">Inspect run</Button></div>
      </header>
      <div className="grid min-h-[694px] lg:grid-cols-[1fr_280px]">
        <main className="gavel-grid relative bg-paper-100 p-5 text-ink-950 lg:p-7">
          <div className="mb-4 flex flex-wrap items-center justify-between gap-3"><div><p className="m-0 text-[10px] font-bold uppercase tracking-[0.15em] text-copper-600">Settlement topology</p><h2 className="mt-1 mb-0 text-xl font-semibold tracking-[-0.03em]">Run #STL-4912</h2></div><div className="flex gap-2"><StatusBadge tone="warning">Waiting</StatusBadge><StatusBadge tone="success">Live</StatusBadge></div></div>
          <AuctionFlow className="h-[560px]" />
          <div className="pointer-events-none absolute bottom-10 left-10 rounded-lg border border-paper-200 bg-paper-50/95 px-3 py-2 text-[10px] text-ink-500 shadow-panel backdrop-blur">Drag nodes · Scroll to zoom · Click to inspect</div>
        </main>
        <aside className="border-l border-white/10 p-5">
          <p className="m-0 text-[10px] font-bold uppercase tracking-[0.16em] text-white/65">Selected step</p><h3 className="mt-2 mb-0 text-base font-semibold">Settlement window</h3><p className="mt-2 mb-0 text-xs leading-5 text-white/60">Durable timer holds the candidate recovery period without keeping domain state.</p>
          <dl className="mt-6 space-y-4 text-xs"><div><dt className="text-white/60">State</dt><dd className="mt-1 ml-0 font-mono text-gold-400">WAITING</dd></div><div><dt className="text-white/60">Started</dt><dd className="mt-1 ml-0 font-mono">14:27:44 UTC</dd></div><div><dt className="text-white/60">Deadline</dt><dd className="mt-1 ml-0 font-mono">16:27:44 UTC</dd></div><div><dt className="text-white/60">Attempts</dt><dd className="mt-1 ml-0 font-mono">1 / 3</dd></div></dl>
          <div className="mt-7 border-t border-white/10 pt-5"><p className="m-0 text-[10px] font-bold uppercase tracking-[0.16em] text-white/65">Run health</p><div className="mt-4 space-y-3">{[['Worker pollers', '12', true], ['Schedule latency', '38 ms', true], ['Deadline slack', '1h 42m', true], ['Payment adapter', 'degraded', false]].map(([label, value, ok]) => <div key={String(label)} className="flex items-center justify-between text-xs"><span className="text-white/65">{label}</span><span className={ok ? 'text-moss-400' : 'text-gold-400'}>{value}</span></div>)}</div></div>
        </aside>
      </div>
    </div>
  );
}

function readVariant(): Variant {
  if (typeof window === 'undefined') return 'A';
  const ownValue = new URLSearchParams(window.location.search).get('variant');
  const parentValue = window.parent !== window
    ? new URLSearchParams(window.parent.location.search).get('variant')
    : null;
  const value = ownValue ?? parentValue;
  return variants.includes(value as Variant) ? value as Variant : 'A';
}

export function AuctionOperationsLab() {
  const [variant, setVariant] = useState<Variant>(readVariant);
  const move = (direction: -1 | 1) => {
    const next = variants[(variants.indexOf(variant) + direction + variants.length) % variants.length];
    const url = new URL(window.location.href);
    url.searchParams.set('variant', next);
    window.history.replaceState({}, '', url);
    if (window.parent !== window) {
      const parentUrl = new URL(window.parent.location.href);
      parentUrl.searchParams.set('variant', next);
      window.parent.history.replaceState({}, '', parentUrl);
    }
    setVariant(next);
  };

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.matches('input, textarea, [contenteditable="true"]')) return;
      if (event.key === 'ArrowLeft') move(-1);
      if (event.key === 'ArrowRight') move(1);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [variant]);

  return (
    <div className="min-h-screen min-w-0 bg-paper-100 p-2 pb-24 sm:p-5 sm:pb-24">
      {variant === 'A' && <VariantA />}
      {variant === 'B' && <VariantB />}
      {variant === 'C' && <VariantC />}
      {import.meta.env.DEV && <div className="fixed bottom-5 left-1/2 z-50 flex -translate-x-1/2 items-center gap-1 rounded-full border border-white/10 bg-ink-950 p-1.5 text-white shadow-2xl">
        <button onClick={() => move(-1)} aria-label="Предыдущий вариант" className="grid size-9 place-items-center rounded-full hover:bg-white/10"><ArrowLeft size={16} /></button>
        <div className="min-w-44 px-3 text-center text-xs"><strong>{variant}</strong><span className="mx-2 text-white/25">/</span><span className="text-white/65">{names[variant]}</span></div>
        <button onClick={() => move(1)} aria-label="Следующий вариант" className="grid size-9 place-items-center rounded-full hover:bg-white/10"><ArrowRight size={16} /></button>
      </div>}
    </div>
  );
}
