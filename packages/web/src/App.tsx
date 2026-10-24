import { useCallback, useEffect, useState } from 'react';
import {
  AdminOverviewPage,
  AuctionFlow,
  Button,
  CreateLotPage,
  CustomerDetailPage,
  CustomersManagementPage,
  ErrorState,
  Kpi,
  KpiStrip,
  LiveBidPanel,
  LotsManagementPage,
  OpsPageShell,
  Panel,
  SettlementRecoveryCard,
  Skeleton,
  StatusBadge,
  formatClock,
  formatDuration,
  formatInt,
  formatMoney,
  opsData,
  type OpsSection,
  type RecoveryState,
} from '@gavel/ui';
import { useLotWorkspace } from './graphql/useLotWorkspace';
import { LocalIdentitySwitcher } from './auth/LocalIdentitySwitcher';

const sectionPaths: Record<OpsSection, string> = {
  overview: '/admin',
  lots: '/admin/lots',
  customers: '/admin/customers',
  settlements: '/admin/settlements',
  workflows: '/admin/workflows',
};

const { today, queue, queueTotal, queueTimeCritical, speedmasterBids, health, liveAuctions, bidsNow } = opsData;

function usePathname() {
  const [pathname, setPathname] = useState(window.location.pathname);
  useEffect(() => {
    const update = () => setPathname(window.location.pathname);
    window.addEventListener('popstate', update);
    return () => window.removeEventListener('popstate', update);
  }, []);
  const navigate = useCallback((next: string) => {
    if (next === window.location.pathname) return;
    window.history.pushState({}, '', next);
    setPathname(next);
    window.scrollTo({ top: 0 });
  }, []);
  return { pathname, navigate };
}

export function App() {
  const { pathname, navigate } = usePathname();
  const navigateSection = (section: OpsSection) => navigate(sectionPaths[section]);

  let page;
  if (pathname === '/admin/monitor') {
    page = <LiveMonitorPage onNavigate={navigateSection} onOpenLot={() => navigate('/admin/lots/GV-2841')} />;
  } else if (pathname === '/admin/inbox') {
    page = <OperationsInboxPage onNavigate={navigateSection} navigate={navigate} />;
  } else if (pathname === '/admin/observability') {
    page = <ObservabilityPage onNavigate={navigateSection} />;
  } else if (pathname === '/admin/lots/new') {
    page = <CreateLotPage onBackToLots={() => navigate('/admin/lots')} onPublished={() => navigate('/admin/lots/LOT-D92F')} />;
  } else if (pathname.startsWith('/admin/lots/')) {
    page = <LotWorkspace lotId={decodeURIComponent(pathname.slice('/admin/lots/'.length))} onNavigate={navigateSection} onBack={() => navigate('/admin/lots')} />;
  } else if (pathname === '/admin/lots') {
    page = <LotsManagementPage onNavigate={navigateSection} onCreateLot={() => navigate('/admin/lots/new')} onOpenLot={(id) => navigate(`/admin/lots/${id}`)} />;
  } else if (pathname.startsWith('/admin/customers/')) {
    page = <CustomerDetailPage onNavigate={navigateSection} onBack={() => navigate('/admin/customers')} />;
  } else if (pathname === '/admin/customers') {
    page = <CustomersManagementPage onNavigate={navigateSection} onOpenCustomer={(id) => navigate(`/admin/customers/${id}`)} />;
  } else if (pathname === '/admin/settlements') {
    page = <SettlementsPage onNavigate={navigateSection} />;
  } else if (pathname === '/admin/workflows') {
    page = <WorkflowsPage onNavigate={navigateSection} />;
  } else {
    page = <AdminOverviewPage onNavigate={navigateSection} onOpenMonitor={() => navigate('/admin/monitor')} onOpenInbox={() => navigate('/admin/inbox')} onOpenObservability={() => navigate('/admin/observability')} onOpenSettlement={() => navigate('/admin/settlements')} onOpenCustomer={() => navigate('/admin/customers/USR-0834')} onOpenLot={() => navigate('/admin/lots/GV-2841')} />;
  }

  return <div className="min-h-dvh bg-ground">{page}<LocalIdentitySwitcher /></div>;
}

function LotWorkspace({ lotId, onNavigate, onBack }: { lotId: string; onNavigate: (section: OpsSection) => void; onBack: () => void }) {
  const { data, error, loading, retry, placeBid, bidPending, bidError, refreshError } = useLotWorkspace(lotId);
  const title = data?.title ?? 'Lot';
  const subtitle = loading ? `${lotId} · loading the federated snapshot` : data ? `${lotId} · auction ${data.auction.status.toLowerCase()} · version ${data.auction.version} · ${data.source === 'bff' ? 'federated GraphQL' : 'local demo snapshot'}` : lotId;
  return (
    <OpsPageShell section="lots" onNavigate={onNavigate} title={title} subtitle={subtitle} actions={<Button variant="secondary" onClick={onBack}>All lots</Button>}>
      {loading && (
        <Panel title="Lot" meta="composing Catalog, Auction and Analytics data">
          <div className="grid gap-3 px-5 pb-5"><Skeleton height={32} width={220} /><Skeleton height={14} width={320} /><Skeleton height={160} /></div>
        </Panel>
      )}
      {!loading && error && (
        <Panel title="Lot">
          <ErrorState message={`Couldn't load the lot: ${error.message}`} onRetry={retry} />
          <p className="px-5 pb-5 text-xs text-muted">Check the Cosmo Router endpoint configured by VITE_GRAPHQL_URL.</p>
        </Panel>
      )}
      {!loading && data && (
        <>
          <div className="mb-4 flex flex-wrap items-center gap-3 text-sm text-muted">
            <StatusBadge tone={data.source === 'bff' ? 'success' : 'neutral'}>{data.source === 'bff' ? 'Live data' : 'Demo data'}</StatusBadge>
            <span>{data.bids.length} public bids in this window · ends {data.auction.endsAt}</span>
          </div>
          {(refreshError || bidError) && <p role="alert" className="mb-4 text-sm text-negative">{bidError?.message ?? `Live refresh paused: ${refreshError?.message}`}</p>}
          <LiveBidPanel currency={data.auction.currency} currentPrice={data.auction.currentPrice} minimumBid={data.auction.minimumBid} endsAt={data.auction.endsAt} bids={data.bids} initialMaximum={data.auction.minimumBid + 2_000} status={data.auction.viewerStatus} onPlaceBid={placeBid} biddingDisabled={!data.auction.canBid || data.auction.status !== 'OPEN'} biddingDisabledReason={terminalBidReason(data.auction.status, data.auction.isWinner, data.auction.isSettlementCandidate)} className={bidPending ? 'opacity-90' : undefined} />
        </>
      )}
    </OpsPageShell>
  );
}

export function terminalBidReason(status: 'SCHEDULED' | 'OPEN' | 'SETTLING' | 'SOLD' | 'UNSOLD' | 'CANCELLED', isWinner: boolean, isSettlementCandidate: boolean) {
  if (status === 'SCHEDULED') return 'Bidding opens when the auction starts.';
  if (status === 'SETTLING') return isSettlementCandidate ? 'You are the settlement candidate. Complete payment to finish.' : 'Bidding is closed while settlement is in progress.';
  if (status === 'SOLD') return isWinner ? 'Sold to you. The result is final.' : 'This auction has been sold.';
  if (status === 'UNSOLD') return 'This auction ended without a winner.';
  if (status === 'CANCELLED') return 'This auction was cancelled.';
  return 'Your current identity is not eligible to bid.';
}

function SettlementsPage({ onNavigate }: { onNavigate: (section: OpsSection) => void }) {
  const [state, setState] = useState<RecoveryState>('recovery_required');
  const [notice, setNotice] = useState('Recovery link prepared. No operator action recorded yet.');
  const transition = (next: RecoveryState, message: string) => { setState(next); setNotice(message); };
  const critical = queue[0];
  const timeLeft = formatDuration(critical.dueAt! - today.nowMinutes);
  return (
    <OpsPageShell section="settlements" onNavigate={onNavigate} title="Settlements" subtitle={`${queue.filter((q) => q.target === 'settlements').length} cases need an operator · ${critical.id} expires at ${formatClock(critical.dueAt!)}, ${timeLeft} from now`}>
      <SettlementRecoveryCard state={state} caseId={critical.id} amount="€42,800" closingPrice="€42,800" candidateAlias="Silver Fox" round={state === 'expired' ? 2 : 1} deadline={`${formatClock(critical.dueAt!)} UTC`} timeLeft={timeLeft} recoveryUrl="https://pay.gavel.test/recover/STL-4912?token=••••••••" onCopyLink={() => setNotice('Secure recovery link copied.')} onOpenRecovery={() => transition('processing', 'The customer opened recovery; waiting for the provider outcome.')} />
      <div role="status" className="mt-4 flex flex-col gap-3 panel px-5 py-3 sm:flex-row sm:items-center">
        <p className="flex-1 text-sm text-ink">{notice}</p>
        <div className="flex flex-wrap gap-2">
          <Button variant="secondary" onClick={() => transition('recovery_required', 'Recovery window reopened for the current candidate.')}>Reset</Button>
          <Button variant="secondary" disabled={state === 'succeeded' || state === 'expired'} onClick={() => transition('expired', 'Recovery expired; the workflow advanced to candidate 2.')}>Expire window</Button>
          <Button disabled={state === 'succeeded' || state === 'expired'} onClick={() => transition('succeeded', 'Payment succeeded; winner confirmed and notifications scheduled.')}>Confirm payment</Button>
        </div>
      </div>
    </OpsPageShell>
  );
}

function WorkflowsPage({ onNavigate }: { onNavigate: (section: OpsSection) => void }) {
  const [step, setStep] = useState<'recovery' | 'payment' | 'complete'>('recovery');
  return (
    <OpsPageShell section="workflows" onNavigate={onNavigate} title="Workflows" subtitle="Temporal executions behind the auctions · the settlement workflow for GV-2841 is waiting on the customer">
      <Panel title="Settlement workflow" meta="GV-2841 · running" action={<StatusBadge tone={step === 'complete' ? 'success' : 'warning'}>{step === 'recovery' ? 'Waiting for the customer' : step === 'payment' ? 'Authorising' : 'Completed'}</StatusBadge>}>
        <div className="px-5 pb-5">
          <AuctionFlow />
          <div className="mt-4 flex flex-wrap items-center gap-2">
            <p className="mr-auto text-sm text-muted">Replay a safe operator view of the workflow state.</p>
            <Button variant="secondary" onClick={() => setStep('recovery')}>Recovery</Button>
            <Button variant="secondary" onClick={() => setStep('payment')}>Payment</Button>
            <Button onClick={() => setStep('complete')}>Complete</Button>
          </div>
        </div>
      </Panel>
    </OpsPageShell>
  );
}

function LiveMonitorPage({ onNavigate, onOpenLot }: { onNavigate: (section: OpsSection) => void; onOpenLot: () => void }) {
  return (
    <OpsPageShell section="overview" onNavigate={onNavigate} title="Live monitor" subtitle={`${liveAuctions.now} auctions live · ${formatInt(bidsNow)} accepted bids in the last minute · the Speedmaster carries the most bids`} actions={<Button onClick={onOpenLot}>Open the hottest lot</Button>}>
      <KpiStrip>
        <Kpi label="Command acceptance" value="99.99%" hint="p99 84 ms" />
        <Kpi label="Realtime fan-out" value="128" unit="ms" hint="p99 · healthy" />
        <Kpi label="Bids rejected" value="0.34%" hint="mostly below the minimum" />
      </KpiStrip>
      <Panel className="mt-4" title="Hottest auction" meta="GV-2841">
        <div className="px-5 pb-5"><LiveBidPanel currentPrice={42_800} minimumBid={43_000} endsAt="8 m 42 s" bids={speedmasterBids} totalBids={184} initialMaximum={45_000} status="leading" /></div>
      </Panel>
    </OpsPageShell>
  );
}

function OperationsInboxPage({ onNavigate, navigate }: { onNavigate: (section: OpsSection) => void; navigate: (path: string) => void }) {
  const paths = { settlements: '/admin/settlements', customers: '/admin/customers/USR-0834', lots: '/admin/lots/GV-2839', create: '/admin/lots/new' } as const;
  return (
    <OpsPageShell section="overview" onNavigate={onNavigate} title="Inbox" subtitle={`${queueTotal} open cases · ${queueTimeCritical} time-critical · oldest opened today 11:05`}>
      <Panel title="Needs an operator" meta={`${queueTotal} open`}>
        <ol className="divide-y divide-rule border-t border-rule">
          {queue.map((item) => (
            <li key={item.id}>
              <button type="button" onClick={() => navigate(paths[item.target])} className="grid min-h-12 w-full grid-cols-[8px_minmax(0,1fr)_auto] items-center gap-3 px-5 py-2.5 text-left hover:bg-raised">
                <span aria-hidden="true" className={`size-2 rounded-full ${item.tone === 'danger' ? 'bg-negative' : item.tone === 'warning' ? 'bg-warning' : 'bg-control'}`} />
                <span className="min-w-0">
                  <span className="block truncate text-sm text-ink">{item.kind} <span className="text-muted">· {item.object}</span></span>
                  <span className="block truncate text-xs text-muted"><span className="font-mono">{item.id}</span> · {item.detail}</span>
                </span>
                <span className={`text-right text-xs tabular-nums ${item.tone === 'danger' ? 'font-medium text-negative' : 'text-muted'}`}>{item.dueAt !== null ? `${formatDuration(item.dueAt - today.nowMinutes)} left` : 'No deadline'}</span>
              </button>
            </li>
          ))}
        </ol>
      </Panel>
    </OpsPageShell>
  );
}

function ObservabilityPage({ onNavigate }: { onNavigate: (section: OpsSection) => void }) {
  const degraded = health.filter((row) => !row.ok);
  return (
    <OpsPageShell section="overview" onNavigate={onNavigate} title="Observability" subtitle={degraded.length ? `${degraded[0].name} is degraded: ${degraded[0].value} ${degraded[0].detail} · the other ${health.length - degraded.length} critical paths are healthy` : 'All critical paths healthy'}>
      <Panel title="Critical paths" meta="last 15 minutes">
        <ul className="divide-y divide-rule border-t border-rule">
          {health.map((row) => (
            <li key={row.name} className="grid h-12 grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-4 px-5 text-sm">
              <span className="truncate text-ink">{row.name}<span className="ml-2 text-xs text-muted">{row.detail}</span></span>
              <span className={`tabular-nums ${row.ok ? 'text-ink' : 'font-medium text-negative'}`}>{row.value}</span>
              <StatusBadge tone={row.ok ? 'success' : 'danger'}>{row.ok ? 'Healthy' : 'Degraded'}</StatusBadge>
            </li>
          ))}
        </ul>
      </Panel>
      <p className="mt-3 text-xs text-muted">Hammer volume today {formatMoney(opsData.hammerVolume.now)} · figures refresh every minute.</p>
    </OpsPageShell>
  );
}
