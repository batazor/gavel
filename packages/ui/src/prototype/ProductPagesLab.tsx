// PROTOTYPE: Five product pages, switchable via ?variant=A|B|C|D|E.
import { useEffect, useState } from 'react';
import { ArrowLeft, ArrowRight } from 'lucide-react';
import { AdminOverviewPage } from '../pages/AdminOverviewPage';
import { CreateLotPage } from '../pages/CreateLotPage';
import { CustomerDetailPage } from '../pages/CustomerDetailPage';
import { CustomersManagementPage } from '../pages/CustomersManagementPage';
import { LotsManagementPage } from '../pages/LotsManagementPage';

type Variant = 'A' | 'B' | 'C' | 'D' | 'E';
const variants: Variant[] = ['A', 'B', 'C', 'D', 'E'];
const names: Record<Variant, string> = { A: 'Create lot', B: 'Lots', C: 'Customers', D: 'Customer profile', E: 'Admin overview' };
const pages = { A: CreateLotPage, B: LotsManagementPage, C: CustomersManagementPage, D: CustomerDetailPage, E: AdminOverviewPage };

function readVariant(): Variant {
  if (typeof window === 'undefined') return 'A';
  const own = new URLSearchParams(window.location.search).get('variant');
  const parent = window.parent !== window ? new URLSearchParams(window.parent.location.search).get('variant') : null;
  const value = own ?? parent;
  return variants.includes(value as Variant) ? value as Variant : 'A';
}

export function ProductPagesLab() {
  const [variant, setVariant] = useState<Variant>(readVariant);
  const Page = pages[variant];
  const move = (direction: -1 | 1) => { const next = variants[(variants.indexOf(variant) + direction + variants.length) % variants.length]; const ownUrl = new URL(window.location.href); ownUrl.searchParams.set('variant', next); window.history.replaceState({}, '', ownUrl); if (window.parent !== window) { const parentUrl = new URL(window.parent.location.href); parentUrl.searchParams.set('variant', next); window.parent.history.replaceState({}, '', parentUrl); } window.scrollTo({ top: 0, behavior: 'instant' }); setVariant(next); };
  useEffect(() => { const handle = (event: KeyboardEvent) => { const target = event.target as HTMLElement | null; if (target?.matches('input, textarea, select, [contenteditable="true"]')) return; if (event.key === 'ArrowLeft') move(-1); if (event.key === 'ArrowRight') move(1); }; window.addEventListener('keydown', handle); return () => window.removeEventListener('keydown', handle); }, [variant]);
  return <><Page />{import.meta.env.DEV && <div className="fixed bottom-5 left-1/2 z-50 flex -translate-x-1/2 items-center gap-1 rounded-full border border-white/10 bg-ink-950 p-1.5 text-white shadow-2xl"><button onClick={() => move(-1)} aria-label="Предыдущая страница" className="grid size-9 place-items-center rounded-full hover:bg-white/10"><ArrowLeft size={16} /></button><div className="min-w-44 px-3 text-center text-xs"><strong>{variant}</strong><span className="mx-2 text-white/25">/</span><span className="text-white/65">{names[variant]}</span></div><button onClick={() => move(1)} aria-label="Следующая страница" className="grid size-9 place-items-center rounded-full hover:bg-white/10"><ArrowRight size={16} /></button></div>}</>;
}
