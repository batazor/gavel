import { useMemo, useState } from 'react';
import { MoreHorizontal, Search, X } from 'lucide-react';
import { Avatar } from '../components/Avatar';
import { Button } from '../components/Button';
import { Kpi, KpiStrip } from '../components/MetricCard';
import { StatusBadge, type StatusTone } from '../components/StatusBadge';
import { cn } from '../lib/cn';
import { formatInt, formatMoney, formatPercent } from '../lib/format';
import { blockedReasons, customers, decisionsToday, members, type Customer, type CustomerState, type PaymentState } from '../data/customers';
import { Panel } from '../components/Panel';
import { today } from '../data/ops';
import { OpsPageShell, type OpsSection } from './PageShell';

type Tab = 'All' | CustomerState;
const tabs: Tab[] = ['All', 'Active', 'Review', 'Restricted'];
const stateTone: Record<CustomerState, StatusTone> = { Active: 'success', Review: 'warning', Restricted: 'danger' };
const paymentCopy: Record<PaymentState, string> = { Ready: 'Ready', Missing: 'No instrument', Revoked: 'Revoked' };

/*
  The screen's question: who is blocked from bidding, and why? The strip states the platform figures, the table
  lists the desk's members with the row's one status mark, and the rail shows the selected member with the one primary.
*/
export function CustomersManagementPage({ onNavigate, onOpenCustomer }: { onNavigate?: (section: OpsSection) => void; onOpenCustomer?: (customerId: string) => void } = {}) {
  const [query, setQuery] = useState('');
  const [tab, setTab] = useState<Tab>('All');
  const [selected, setSelected] = useState(customers[0]);
  const counts = useMemo(() => Object.fromEntries(tabs.map((t) => [t, t === 'All' ? customers.length : customers.filter((c) => c.state === t).length])) as Record<Tab, number>, []);
  const filtered = useMemo(() => customers.filter((customer) => (tab === 'All' || customer.state === tab) && `${customer.name} ${customer.email} ${customer.id}`.toLowerCase().includes(query.toLowerCase())), [query, tab]);
  const blocked = customers.filter((c) => c.state !== 'Active' || c.payment !== 'Ready').length;

  return (
    <OpsPageShell
      section="customers"
      onNavigate={onNavigate}
      title="Customers"
      subtitle={`${formatInt(members.needReview)} members platform-wide are waiting for review · ${blocked} of the ${customers.length} on the ${today.desk} cannot bid right now`}
      actions={<Button variant="secondary">Invite operator</Button>}
    >
      <KpiStrip>
        <Kpi label="Registered members" value={formatInt(members.total)} hint="all regions" />
        <Kpi label="Ready to bid" value={formatInt(members.readyToBid)} hint={`${formatPercent(members.readyToBid / members.total)} of members`} />
        <Kpi label="Waiting for review" value={formatInt(members.needReview)} hint={`${members.needReviewHighPriority} high priority`} />
      </KpiStrip>

      <div role="tablist" aria-label="Member states" className="mt-6 flex gap-6 overflow-x-auto border-b border-line">
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
              <input aria-label="Search members" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Name, email or member ID" className="min-w-0 flex-1 border-0 bg-transparent text-sm text-ink outline-none placeholder:text-faint" />
              {query && <button type="button" aria-label="Clear search" onClick={() => setQuery('')} className="text-muted hover:text-ink"><X size={14} /></button>}
            </label>
            <span className="ml-auto text-sm text-muted tabular-nums">{filtered.length} of {customers.length} members</span>
          </div>
          <table className="mt-3 w-full text-sm">
            <thead>
              <tr className="text-xs text-muted">
                <th className="h-9 w-[34%] pr-3 text-left font-normal">Member</th>
                <th className="hidden h-9 px-3 text-left font-normal md:table-cell">Payment</th>
                <th className="hidden h-9 px-3 text-right font-normal lg:table-cell">Bids</th>
                <th className="hidden h-9 px-3 text-right font-normal lg:table-cell">Won</th>
                <th className="h-9 px-3 text-right font-normal">Won volume</th>
                <th className="hidden h-9 px-3 text-left font-normal xl:table-cell">Last active</th>
                <th className="hidden h-9 px-3 text-left font-normal sm:table-cell">State</th>
                <th className="h-9 w-10"><span className="sr-only">Actions</span></th>
              </tr>
            </thead>
            <tbody className="divide-y divide-rule border-t border-line">
              {filtered.map((customer) => (
                <tr key={customer.id} tabIndex={0} aria-selected={selected.id === customer.id} onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); setSelected(customer); } }} onClick={() => setSelected(customer)} className={cn('h-14 cursor-pointer focus-visible:outline-2 focus-visible:-outline-offset-2', selected.id === customer.id && 'bg-active')}>
                  <td className="max-w-0 pr-3">
                    <span className="flex items-center gap-3">
                      <Avatar name={customer.name} hue={customer.hue} />
                      <span className="min-w-0">
                        <span className="block truncate font-medium text-ink">{customer.name}</span>
                        <span className="block truncate text-xs text-muted"><span className="sm:hidden">{customer.state} · </span>{customer.email}<span className="md:hidden"> · {paymentCopy[customer.payment]}</span></span>
                      </span>
                    </span>
                  </td>
                  <td className={cn('hidden px-3 md:table-cell', customer.payment === 'Ready' ? 'text-ink' : customer.payment === 'Revoked' ? 'text-negative' : 'text-muted')}>{paymentCopy[customer.payment]}</td>
                  <td className="hidden px-3 text-right tabular-nums lg:table-cell">{customer.bids === 0 ? <span className="text-faint">0</span> : formatInt(customer.bids)}</td>
                  <td className="hidden px-3 text-right tabular-nums lg:table-cell">{customer.won === 0 ? <span className="text-faint">0</span> : formatInt(customer.won)}</td>
                  <td className="px-3 text-right font-bold tabular-nums">{customer.volume === 0 ? <span className="font-normal text-faint">€0</span> : formatMoney(customer.volume)}</td>
                  <td className="hidden px-3 whitespace-nowrap text-muted xl:table-cell">{customer.lastSeen}</td>
                  <td className="hidden px-3 sm:table-cell"><StatusBadge tone={stateTone[customer.state]}>{customer.state}</StatusBadge></td>
                  <td className="w-10 text-right"><button type="button" aria-label={`Open ${customer.name}`} onClick={(event) => { event.stopPropagation(); setSelected(customer); onOpenCustomer?.(customer.id); }} className="grid size-8 place-items-center rounded-lg text-muted hover:bg-raised hover:text-ink"><MoreHorizontal size={16} /></button></td>
                </tr>
              ))}
            </tbody>
          </table>
          {filtered.length === 0 && <p className="mt-3 grid min-h-20 place-items-center rounded-lg border border-dashed border-control text-sm text-muted">No members match "{query}" under {tab}.</p>}
        </div>
        <CustomerPeek customer={selected} onOpen={onOpenCustomer ? () => onOpenCustomer(selected.id) : undefined} />
      </div>

      <div className="mt-5 grid items-start gap-4 lg:grid-cols-2">
        <Panel title="Why members cannot bid" meta={`${formatInt(members.total - members.readyToBid)} platform-wide`}>
          <ol className="divide-y divide-rule border-t border-rule">
            {blockedReasons.map((row, index) => {
              const share = row.count / blockedReasons[0].count;
              return (
                <li key={row.reason} className="grid h-11 grid-cols-[minmax(0,1fr)_72px] items-center gap-4 px-5 text-sm">
                  <span className="min-w-0">
                    <span className="block truncate text-ink">{row.reason}</span>
                    <span aria-hidden="true" className={cn('mt-1 block h-0.5 rounded-full', index === 0 ? 'bg-ink' : 'bg-control')} style={{ width: `${Math.max(2, share * 100)}%` }} />
                  </span>
                  <span className="text-right text-ink tabular-nums">{formatInt(row.count)}</span>
                </li>
              );
            })}
          </ol>
          <p className="px-5 py-3 text-xs text-muted">Reviews are the smallest group and the only one an operator can clear today.</p>
        </Panel>
        <Panel title="Decisions today" meta={`${decisionsToday.length} on the ${today.desk}`}>
          <ol className="divide-y divide-rule border-t border-rule">
            {decisionsToday.map((row) => (
              <li key={row.id} className="grid h-11 grid-cols-[48px_minmax(0,1fr)_auto] items-center gap-3 px-5 text-sm">
                <span className="text-xs text-muted tabular-nums">{row.time}</span>
                <span className="min-w-0 truncate text-ink">{row.member}<span className="ml-2 font-mono text-xs text-faint">{row.id}</span><span className="ml-2 hidden text-xs text-muted sm:inline">by {row.by}</span></span>
                <span className={cn('text-xs', row.decision === 'Restricted' ? 'text-negative' : 'text-muted')}>{row.decision}</span>
              </li>
            ))}
          </ol>
        </Panel>
      </div>
    </OpsPageShell>
  );
}

function CustomerPeek({ customer, onOpen }: { customer: Customer; onOpen?: () => void }) {
  return (
    <aside className="panel p-4 xl:sticky xl:top-6">
      <div className="flex items-start gap-3">
        <Avatar name={customer.name} hue={customer.hue} size="lg" />
        <div className="min-w-0">
          <h2 className="text-base font-medium text-ink">{customer.name}</h2>
          <p className="truncate text-xs text-muted">{customer.email}</p>
          <p className="font-mono text-xs text-muted">{customer.id}</p>
        </div>
      </div>
      <dl className="mt-4 divide-y divide-rule text-sm">
        <Fact label="Won volume" value={customer.volume ? formatMoney(customer.volume) : '€0'} money={customer.volume > 0} />
        <Fact label="Bids · won" value={`${formatInt(customer.bids)} · ${formatInt(customer.won)}`} />
        <Fact label="Payment" value={paymentCopy[customer.payment]} />
        <Fact label="Member since" value={customer.since} />
        <Fact label="Last active" value={customer.lastSeen} />
      </dl>
      {customer.state === 'Review' && <p className="mt-4 rounded-lg bg-pill-warning px-3 py-2 text-xs text-pill-warning-ink">Review opened today: bid velocity tripled, no failed settlement.</p>}
      {customer.payment === 'Missing' && <p className="mt-4 rounded-lg bg-raised px-3 py-2 text-xs text-muted">No payment instrument on file; the member can watch but not bid.</p>}
      <Button className="mt-4 w-full" onClick={onOpen} disabled={!onOpen}>Open member</Button>
    </aside>
  );
}

function Fact({ label, value, money }: { label: string; value: string; money?: boolean }) {
  return <div className="flex h-9 items-center justify-between gap-3"><dt className="text-muted">{label}</dt><dd className={cn('text-right text-ink tabular-nums', money && 'font-bold')}>{value}</dd></div>;
}

export { Avatar, customers };
export type { Customer };
