import { useMemo, useState, type ReactNode } from 'react';
import { CheckCircle2, ChevronRight, CreditCard, Mail, MoreHorizontal, Search, ShieldAlert, ShieldCheck, UserPlus, Users } from 'lucide-react';
import { Button } from '../components/Button';
import { MetricCard } from '../components/MetricCard';
import { Panel } from '../components/Panel';
import { StatusBadge, type StatusTone } from '../components/StatusBadge';
import { cn } from '../lib/cn';
import { OpsPageShell, type OpsSection } from './PageShell';

type CustomerState = 'Active' | 'Review' | 'Restricted';
type Customer = { id: string; name: string; email: string; since: string; state: CustomerState; payment: 'Ready' | 'Missing' | 'Revoked'; bids: number; won: number; volume: string; lastSeen: string };
const customers: Customer[] = [
  { id: 'USR-0834', name: 'Marta Weiss', email: 'marta.w@example.com', since: 'Mar 2025', state: 'Review', payment: 'Ready', bids: 128, won: 7, volume: '€84,200', lastSeen: '2m ago' },
  { id: 'USR-0811', name: 'Jon Bell', email: 'jon.b@example.com', since: 'Feb 2025', state: 'Active', payment: 'Ready', bids: 64, won: 3, volume: '€31,800', lastSeen: '18m ago' },
  { id: 'USR-0772', name: 'Aya Morita', email: 'aya.m@example.com', since: 'Dec 2024', state: 'Active', payment: 'Ready', bids: 203, won: 11, volume: '€126,400', lastSeen: '1h ago' },
  { id: 'USR-0691', name: 'Liam Costa', email: 'liam.c@example.com', since: 'Oct 2024', state: 'Restricted', payment: 'Revoked', bids: 18, won: 1, volume: '€4,800', lastSeen: '3d ago' },
  { id: 'USR-0528', name: 'Nora Lind', email: 'nora.l@example.com', since: 'Jun 2024', state: 'Active', payment: 'Missing', bids: 0, won: 0, volume: '€0', lastSeen: '5d ago' },
];
const stateTone: Record<CustomerState, StatusTone> = { Active: 'success', Review: 'warning', Restricted: 'danger' };

export function CustomersManagementPage({ onNavigate, onOpenCustomer }: { onNavigate?: (section: OpsSection) => void; onOpenCustomer?: (customerId: string) => void } = {}) {
  const [query, setQuery] = useState('');
  const [segment, setSegment] = useState<'All' | CustomerState>('All');
  const [selected, setSelected] = useState(customers[0]);
  const filtered = useMemo(() => customers.filter((customer) => (segment === 'All' || customer.state === segment) && `${customer.name} ${customer.email} ${customer.id}`.toLowerCase().includes(query.toLowerCase())), [query, segment]);
  return <OpsPageShell section="customers" onNavigate={onNavigate} title="Customers" subtitle="Identity, payment readiness and auction participation" actions={<Button size="sm" variant="secondary" leadingIcon={<UserPlus size={15} />}>Invite operator</Button>}>
    <div className="grid gap-4 sm:grid-cols-3"><MetricCard label="Registered members" value="24,812" change={6.4} hint="30 days" icon={<Users size={18} />} /><MetricCard label="Ready to bid" value="18,405" hint="74.2% of members" icon={<CreditCard size={18} />} /><MetricCard label="Needs review" value="38" hint="12 high priority" icon={<ShieldAlert size={18} />} /></div>
    <div className="mt-5 grid gap-5 xl:grid-cols-[1fr_310px]">
      <Panel className="min-w-0"><div className="flex flex-col gap-3 border-b border-paper-200 p-4 sm:flex-row"><label className="flex min-w-0 flex-1 items-center gap-2 rounded-lg border border-paper-200 bg-white px-3 py-2 text-ink-500"><Search size={15} /><input aria-label="Search customers" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Name, email or customer ID" className="min-w-0 flex-1 border-0 bg-transparent text-sm outline-none" /></label><div className="flex gap-2">{(['All', 'Active', 'Review', 'Restricted'] as const).map((item) => <button key={item} onClick={() => setSegment(item)} className={cn('rounded-lg border px-3 py-2 text-xs font-semibold', segment === item ? 'border-ink-950 bg-ink-950 text-white' : 'border-paper-200 bg-white text-ink-700')}>{item}</button>)}</div></div>
        <div className="overflow-x-auto"><table className="w-full min-w-[560px] border-collapse text-left"><thead><tr className="border-b border-paper-200 bg-paper-100/60 text-[10px] uppercase tracking-[0.12em] text-ink-500"><th className="px-3 py-3">Customer</th><th className="px-3 py-3">Payment</th><th className="hidden px-3 py-3 lg:table-cell">Participation</th><th className="px-3 py-3">Won volume</th><th className="px-3 py-3">State</th><th className="px-3 py-3"><span className="sr-only">Actions</span></th></tr></thead><tbody>{filtered.map((customer) => <tr key={customer.id} tabIndex={0} onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') setSelected(customer); }} onClick={() => setSelected(customer)} className={cn('cursor-pointer border-b border-paper-200/70 last:border-0 hover:bg-paper-100/55 focus-visible:outline-2 focus-visible:outline-inset focus-visible:outline-copper-500', selected.id === customer.id && 'bg-copper-500/5 shadow-[inset_3px_0_0_#bd5634]')}><td className="px-3 py-3"><div className="flex items-center gap-2"><Avatar name={customer.name} /><div><div className="text-sm font-semibold">{customer.name}</div><div className="mt-1 max-w-36 truncate text-[10px] text-ink-500">{customer.email} · {customer.id}</div></div></div></td><td className="px-3 py-3"><PaymentState state={customer.payment} /></td><td className="hidden px-3 py-3 text-xs lg:table-cell"><strong>{customer.bids}</strong> bids · <strong>{customer.won}</strong> won</td><td className="px-3 py-3 text-sm font-bold tabular-nums">{customer.volume}</td><td className="px-3 py-3"><StatusBadge tone={stateTone[customer.state]}>{customer.state}</StatusBadge></td><td className="px-3 py-3"><button aria-label={`Actions for ${customer.name}`} className="rounded p-2 text-ink-500 hover:bg-paper-200"><MoreHorizontal size={16} /></button></td></tr>)}</tbody></table></div>
      </Panel>
      <CustomerPeek customer={selected} onOpen={onOpenCustomer ? () => onOpenCustomer(selected.id) : undefined} />
    </div>
  </OpsPageShell>;
}

export function Avatar({ name, large = false }: { name: string; large?: boolean }) {
  const initials = name.split(' ').map((part) => part[0]).join('').slice(0, 2);
  return <span className={cn('grid shrink-0 place-items-center rounded-full bg-ink-950 font-bold text-paper-50', large ? 'size-16 text-lg' : 'size-10 text-xs')}>{initials}</span>;
}

function PaymentState({ state }: { state: Customer['payment'] }) {
  return <span className={cn('inline-flex items-center gap-1.5 text-xs font-semibold', state === 'Ready' ? 'text-moss-600' : state === 'Revoked' ? 'text-red-700' : 'text-ink-500')}>{state === 'Ready' ? <CheckCircle2 size={14} /> : state === 'Revoked' ? <ShieldAlert size={14} /> : <CreditCard size={14} />}{state}</span>;
}

function CustomerPeek({ customer, onOpen }: { customer: Customer; onOpen?: () => void }) {
  return <aside className="h-fit rounded-xl border border-paper-200 bg-paper-50 p-5 shadow-panel xl:sticky xl:top-5"><div className="flex items-start gap-3"><Avatar name={customer.name} large /><div className="min-w-0"><h2 className="m-0 text-lg font-semibold">{customer.name}</h2><p className="mt-1 mb-0 truncate text-xs text-ink-500">{customer.email}</p><p className="mt-1 mb-0 font-mono text-[9px] text-ink-500">{customer.id}</p></div></div><div className="mt-5 grid grid-cols-2 gap-2"><SmallStat label="Won" value={String(customer.won)} /><SmallStat label="Volume" value={customer.volume} /></div><dl className="mt-5 space-y-3 text-xs"><PeekFact icon={<CreditCard size={14} />} label="Payment instrument" value={customer.payment} /><PeekFact icon={<ShieldCheck size={14} />} label="Platform access" value={customer.state === 'Restricted' ? 'Restricted' : 'Member'} /><PeekFact icon={<Mail size={14} />} label="Last active" value={customer.lastSeen} /></dl>{customer.state === 'Review' && <div className="mt-5 rounded-lg border border-amber-700/20 bg-amber-500/10 p-3 text-xs leading-5 text-amber-900"><strong>Manual review</strong><br />High-value bidding velocity changed significantly today.</div>}<Button className="mt-5 w-full" onClick={onOpen}>Open customer <ChevronRight size={14} /></Button></aside>;
}

function SmallStat({ label, value }: { label: string; value: string }) { return <div className="rounded-lg bg-paper-100 p-3"><p className="m-0 text-[9px] font-bold uppercase tracking-wider text-ink-500">{label}</p><p className="mt-1 mb-0 text-lg font-bold">{value}</p></div>; }
function PeekFact({ icon, label, value }: { icon: ReactNode; label: string; value: string }) { return <div className="flex items-center gap-2"><span className="text-ink-500">{icon}</span><dt className="flex-1 text-ink-500">{label}</dt><dd className="m-0 font-semibold">{value}</dd></div>; }

export type { Customer };
export { customers };
