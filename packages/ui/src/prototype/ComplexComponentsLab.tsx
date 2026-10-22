// PROTOTYPE: Three domain-heavy component contexts, switchable via ?variant=A|B|C.
import { useEffect, useState } from 'react';
import { ArrowLeft, ArrowRight, Check, Gavel, PackageCheck, ShieldCheck } from 'lucide-react';
import { Button } from '../components/Button';
import { ClosingPolicyBuilder } from '../components/ClosingPolicyBuilder';
import { LiveBidPanel, type PublicBid } from '../components/LiveBidPanel';
import { SettlementRecoveryCard } from '../components/SettlementRecoveryCard';
import { StatusBadge } from '../components/StatusBadge';

type Variant = 'A' | 'B' | 'C';
const variants: Variant[] = ['A', 'B', 'C'];
const names: Record<Variant, string> = { A: 'Live bidding', B: 'Closing policy', C: 'Settlement recovery' };
const bids: PublicBid[] = [
  { id: 'b-184', alias: 'Silver Fox', resultingPrice: 42_800, occurredAt: '14:31:52', isMine: true },
  { id: 'b-183', alias: 'Atlas 17', resultingPrice: 42_600, occurredAt: '14:31:49' },
  { id: 'b-182', alias: 'North Star', resultingPrice: 42_100, occurredAt: '14:30:08' },
  { id: 'b-181', alias: 'Silver Fox', resultingPrice: 41_800, occurredAt: '14:29:44', isMine: true },
  { id: 'b-180', alias: 'Mistral 4', resultingPrice: 41_500, occurredAt: '14:28:16' },
];

function Header({ context }: { context: string }) {
  return <header className="flex h-16 items-center border-b border-paper-200 bg-paper-50 px-5 lg:px-8"><div className="flex items-center gap-2 font-bold tracking-[0.1em]"><Gavel size={18} className="text-copper-500" /> GAVEL</div><span className="mx-4 h-5 w-px bg-paper-200" /><span className="text-xs text-ink-500">{context}</span><Button variant="ghost" size="sm" className="ml-auto">Elena V.</Button></header>;
}

function BidContext() {
  return <div className="min-h-[820px] bg-paper-100"><Header context="Live auction" /><main className="mx-auto grid max-w-6xl gap-7 px-5 py-8 lg:grid-cols-[0.8fr_1.2fr] lg:px-8"><section><div className="relative aspect-[4/3] overflow-hidden rounded-2xl bg-[radial-gradient(circle_at_60%_35%,#7f7a70_0,#302f2c_45%,#171716_75%)] shadow-panel"><div className="absolute inset-0 grid place-items-center"><div className="grid size-40 place-items-center rounded-full border border-white/15 bg-white/5 text-paper-200/65 backdrop-blur"><Gavel size={72} strokeWidth={1} /></div></div><span className="absolute top-4 left-4"><StatusBadge tone="success">Lot GV-2841</StatusBadge></span><span className="absolute right-4 bottom-4 text-[10px] uppercase tracking-[0.12em] text-white/65">1 / 8 photos</span></div><p className="mt-6 mb-1 text-[10px] font-bold uppercase tracking-[0.16em] text-copper-600">Watches · No reserve</p><h1 className="mt-0 mb-2 text-3xl font-semibold tracking-[-0.04em]">Omega Speedmaster, 1969</h1><p className="m-0 text-sm leading-6 text-ink-500">Calibre 861, original stepped dial, recently serviced. Includes period-correct bracelet and archive extract.</p><div className="mt-5 flex gap-5 border-t border-paper-200 pt-4 text-xs"><span><strong>Condition</strong><br /><span className="text-ink-500">Very good</span></span><span><strong>Location</strong><br /><span className="text-ink-500">Berlin, DE</span></span></div></section><LiveBidPanel currentPrice={42_800} minimumBid={43_000} endsAt="08:42 left" bids={bids} initialMaximum={45_000} status="leading" /></main></div>;
}

function PolicyContext() {
  return <div className="min-h-[820px] bg-paper-100"><Header context="Create lot" /><main className="mx-auto max-w-6xl px-5 py-8 lg:px-8"><div className="mb-7 flex items-start justify-between"><div><p className="m-0 text-[10px] font-bold uppercase tracking-[0.15em] text-copper-600">Step 3 of 4</p><h1 className="mt-1 mb-0 text-3xl font-semibold tracking-[-0.04em]">Pricing & timing</h1></div><StatusBadge tone="neutral">Draft saved</StatusBadge></div><div className="grid gap-6 lg:grid-cols-[1fr_380px]"><ClosingPolicyBuilder initialValue={{ scheduledEnd: '2026-10-24T18:00', inactivityMinutes: 5, hardDeadline: '2026-10-24T20:00' }} /><aside className="space-y-5"><section className="rounded-2xl border border-paper-200 bg-paper-50 p-5 shadow-panel"><p className="m-0 text-[10px] font-bold uppercase tracking-[0.14em] text-ink-500">Lot readiness</p><div className="mt-4 space-y-3 text-xs"><div className="flex items-center gap-2"><Check size={14} className="text-moss-600" /> 8 images ready</div><div className="flex items-center gap-2"><Check size={14} className="text-moss-600" /> Description complete</div><div className="flex items-center gap-2"><Check size={14} className="text-moss-600" /> Starting price €18,000</div><div className="flex items-center gap-2"><ShieldCheck size={14} className="text-moss-600" /> Seller verified</div></div></section><section className="rounded-2xl bg-ink-950 p-5 text-paper-50"><PackageCheck size={24} className="text-copper-400" /><h2 className="mt-4 mb-1 text-base font-semibold">What happens next?</h2><p className="m-0 text-xs leading-5 text-white/65">Publishing creates the catalog lot. Auction opens asynchronously when the LotListed event arrives.</p><Button className="mt-5 w-full">Review & publish</Button></section></aside></div></main></div>;
}

function SettlementContext() {
  return <div className="min-h-[820px] bg-paper-100"><Header context="Settlement operations" /><main className="mx-auto max-w-6xl px-5 py-8 lg:px-8"><div className="mb-7 flex flex-wrap items-end justify-between gap-4"><div><p className="m-0 text-[10px] font-bold uppercase tracking-[0.15em] text-copper-600">Exception workspace</p><h1 className="mt-1 mb-0 text-3xl font-semibold tracking-[-0.04em]">Payment recovery</h1></div><div className="flex gap-2"><Button variant="secondary">Assign case</Button><Button variant="ghost">Open workflow</Button></div></div><SettlementRecoveryCard state="recovery_required" caseId="STL-4912" amount="€42,800" closingPrice="€42,800" candidateAlias="Silver Fox" round={1} deadline="16:27 UTC" recoveryUrl="https://pay.gavel.test/recover/STL-4912?token=••••••••" /></main></div>;
}

function readVariant(): Variant {
  if (typeof window === 'undefined') return 'A';
  const own = new URLSearchParams(window.location.search).get('variant');
  const parent = window.parent !== window ? new URLSearchParams(window.parent.location.search).get('variant') : null;
  const value = own ?? parent;
  return variants.includes(value as Variant) ? value as Variant : 'A';
}

export function ComplexComponentsLab() {
  const [variant, setVariant] = useState<Variant>(readVariant);
  const move = (direction: -1 | 1) => {
    const next = variants[(variants.indexOf(variant) + direction + variants.length) % variants.length];
    const ownUrl = new URL(window.location.href); ownUrl.searchParams.set('variant', next); window.history.replaceState({}, '', ownUrl);
    if (window.parent !== window) { const parentUrl = new URL(window.parent.location.href); parentUrl.searchParams.set('variant', next); window.parent.history.replaceState({}, '', parentUrl); }
    setVariant(next);
  };
  useEffect(() => { const handle = (event: KeyboardEvent) => { const target = event.target as HTMLElement | null; if (target?.matches('input, textarea, [contenteditable="true"]')) return; if (event.key === 'ArrowLeft') move(-1); if (event.key === 'ArrowRight') move(1); }; window.addEventListener('keydown', handle); return () => window.removeEventListener('keydown', handle); }, [variant]);
  return <div>{variant === 'A' && <BidContext />}{variant === 'B' && <PolicyContext />}{variant === 'C' && <SettlementContext />}{import.meta.env.DEV && <div className="fixed bottom-5 left-1/2 z-50 flex -translate-x-1/2 items-center gap-1 rounded-full border border-white/10 bg-ink-950 p-1.5 text-white shadow-2xl"><button onClick={() => move(-1)} aria-label="Предыдущий компонент" className="grid size-9 place-items-center rounded-full hover:bg-white/10"><ArrowLeft size={16} /></button><div className="min-w-44 px-3 text-center text-xs"><strong>{variant}</strong><span className="mx-2 text-white/25">/</span><span className="text-white/65">{names[variant]}</span></div><button onClick={() => move(1)} aria-label="Следующий компонент" className="grid size-9 place-items-center rounded-full hover:bg-white/10"><ArrowRight size={16} /></button></div>}</div>;
}
