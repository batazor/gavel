import { useState } from 'react';
import { Avatar } from '../components/Avatar';
import { Button } from '../components/Button';
import { Kpi, KpiStrip } from '../components/MetricCard';
import { Panel } from '../components/Panel';
import { StatusBadge } from '../components/StatusBadge';
import { cn } from '../lib/cn';
import { formatInt, formatMoney } from '../lib/format';
import { customers } from '../data/customers';
import { OpsPageShell, type OpsSection } from './PageShell';

type Tab = 'overview' | 'activity' | 'access';
const tabs: Array<{ key: Tab; label: string }> = [{ key: 'overview', label: 'Overview' }, { key: 'activity', label: 'Activity' }, { key: 'access', label: 'Access' }];

/*
  The screen's question: should this member keep bidding? The object is the header; the decision is the primary;
  the evidence (bids, settlements, participation) is the body.
*/
export function CustomerDetailPage({ onNavigate, onBack }: { onNavigate?: (section: OpsSection) => void; onBack?: () => void } = {}) {
  const customer = customers[0];
  const [tab, setTab] = useState<Tab>('overview');
  const [decision, setDecision] = useState<'review' | 'approved' | 'restricted'>('review');
  const state = decision === 'approved' ? 'Active' : decision === 'restricted' ? 'Restricted' : 'Review';

  return (
    <OpsPageShell
      section="customers"
      onNavigate={onNavigate}
      title={customer.name}
      subtitle={decision === 'review' ? 'Review opened today at 11:05: bid velocity tripled across three live lots. No failed settlement on record.' : decision === 'approved' ? 'Review closed: access confirmed, the member can keep bidding.' : 'Review closed: the account is restricted and open bids are withdrawn.'}
      actions={<>
        <Button variant="ghost" onClick={onBack}>All members</Button>
        <Button variant="secondary">Send message</Button>
        {decision === 'review' && <Button variant="danger" onClick={() => setDecision('restricted')}>Restrict</Button>}
        {decision === 'review' && <Button onClick={() => setDecision('approved')}>Approve</Button>}
      </>}
    >
      <div className="flex flex-wrap items-center gap-4 panel px-5 py-4">
        <Avatar name={customer.name} hue={customer.hue} size="lg" />
        <div className="min-w-0 flex-1">
          <p className="text-sm text-ink"><span className="font-mono">{customer.id}</span> · member since {customer.since} · {customer.region} · {customer.email}</p>
          <p className="mt-0.5 text-xs text-muted">Signed in with Google · last active {customer.lastSeen}</p>
        </div>
        <div className="flex gap-2">
          <StatusBadge tone={state === 'Active' ? 'success' : state === 'Restricted' ? 'danger' : 'warning'}>{state}</StatusBadge>
          <StatusBadge tone="success">Payment ready</StatusBadge>
        </div>
      </div>

      <div role="tablist" aria-label="Member sections" className="mt-6 flex gap-6 border-b border-line">
        {tabs.map((item) => (
          <button key={item.key} role="tab" aria-selected={tab === item.key} onClick={() => setTab(item.key)} className={cn('relative py-2.5 text-sm transition-colors', tab === item.key ? 'text-ink after:absolute after:right-0 after:bottom-[-1px] after:left-0 after:h-0.5 after:bg-ink' : 'text-muted hover:text-ink')}>{item.label}</button>
        ))}
      </div>

      <div className="mt-5">
        {tab === 'overview' && <OverviewTab />}
        {tab === 'activity' && <ActivityTab />}
        {tab === 'access' && <AccessTab />}
      </div>
    </OpsPageShell>
  );
}

function OverviewTab() {
  const customer = customers[0];
  const participation = [
    { title: 'Omega Speedmaster, 1969', id: 'GV-2841', state: 'Leading', amount: 'Maximum €45,000' },
    { title: 'Eames lounge chair', id: 'GV-2839', state: 'Outbid', amount: 'Maximum €7,800' },
    { title: 'Leica M6 Titanium', id: 'GV-2836', state: 'Watching', amount: 'No bid' },
  ];
  return (
    <div className="grid gap-4">
      <KpiStrip>
        <Kpi label="Bids placed" value={formatInt(customer.bids)} hint="across 26 auctions" />
        <Kpi label="Lots won" value={formatInt(customer.won)} hint={`${formatMoney(customer.volume)} hammer volume`} />
        <Kpi label="Settled" value={`${customer.won} of ${customer.won}`} hint="every win paid inside the window" />
      </KpiStrip>
      <div className="grid items-start gap-4 lg:grid-cols-2">
        <Panel title="Payment readiness" meta="Payments">
          <dl className="divide-y divide-rule border-t border-rule px-5 text-sm">
            <Row label="Instrument" value="Visa ending 4242" />
            <Row label="Authorisation" value="v3, prepared 18 Sep 2026" />
            <Row label="Debit consent" value="Accepted" />
            <Row label="Recovery email" value={customer.email} />
          </dl>
        </Panel>
        <Panel title="Live participation" meta={`${participation.length} lots`}>
          <ul className="divide-y divide-rule border-t border-rule">
            {participation.map((row) => (
              <li key={row.id} className="flex h-10 items-center gap-3 px-5 text-sm">
                <span className="min-w-0 flex-1 truncate text-ink">{row.title}<span className="ml-2 font-mono text-xs text-faint">{row.id}</span></span>
                <span className="hidden text-xs text-muted sm:inline">{row.amount}</span>
                <StatusBadge tone={row.state === 'Leading' ? 'success' : row.state === 'Outbid' ? 'warning' : 'neutral'}>{row.state}</StatusBadge>
              </li>
            ))}
          </ul>
          <p className="px-5 py-3 text-xs text-muted">Maximums are private to the member; operators see them only here.</p>
        </Panel>
      </div>
      <Panel title="Operator note" meta="internal">
        <p className="px-5 pb-5 text-sm text-ink">Long-time collector. Review was triggered by unusual bid velocity, not by a failed settlement.</p>
      </Panel>
    </div>
  );
}

function ActivityTab() {
  const events = [['14:31:52', 'Bid accepted', 'GV-2841 · resulting price €42,800'], ['14:29:44', 'Bid accepted', 'GV-2841 · resulting price €41,800'], ['11:05:18', 'Review opened', 'Bid velocity tripled'], ['11:05:02', 'Payment instrument checked', 'Ready · authorisation active'], ['Yesterday', 'Signed in', 'Google · Berlin, DE']];
  return (
    <Panel title="Activity" meta="audit-safe view">
      <ol className="divide-y divide-rule border-t border-rule">
        {events.map(([time, title, detail]) => (
          <li key={`${time}-${title}`} className="grid min-h-11 grid-cols-[80px_minmax(0,1fr)] items-center gap-3 px-5 py-2 text-sm">
            <span className="text-xs text-muted tabular-nums">{time}</span>
            <span className="min-w-0 truncate text-ink">{title}<span className="ml-2 text-xs text-muted">{detail}</span></span>
          </li>
        ))}
      </ol>
    </Panel>
  );
}

function AccessTab() {
  const permissions: Array<[string, string, boolean]> = [['platform:gavel', 'member', true], ['auction:GV-2841', 'place_bid', true], ['auction:GV-2841', 'view_personal_state', true], ['lot:GV-2841', 'edit', false]];
  return (
    <Panel title="Effective permissions" meta="SpiceDB">
      <p className="px-5 pb-3 text-sm text-muted">Permissions follow resource relationships. Payment readiness is checked separately by the domain.</p>
      <table className="w-full text-sm">
        <thead><tr className="border-t border-rule text-xs text-muted"><th className="h-9 pl-5 text-left font-normal">Resource</th><th className="h-9 text-left font-normal">Permission</th><th className="h-9 pr-5 text-right font-normal">Result</th></tr></thead>
        <tbody className="divide-y divide-rule border-t border-rule">
          {permissions.map(([resource, permission, allowed]) => (
            <tr key={`${resource}-${permission}`} className="h-10"><td className="pl-5 font-mono text-xs text-ink">{resource}</td><td className="text-ink">{permission}</td><td className={cn('pr-5 text-right', allowed ? 'text-ink' : 'text-negative')}>{allowed ? 'Allowed' : 'Denied'}</td></tr>
          ))}
        </tbody>
      </table>
    </Panel>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return <div className="flex h-10 items-center justify-between gap-3"><dt className="text-muted">{label}</dt><dd className="truncate text-right text-ink">{value}</dd></div>;
}
