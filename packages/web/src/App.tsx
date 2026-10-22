import { useCallback, useEffect, useState } from 'react';
import {
  AdminOverviewPage,
  AuctionFlow,
  Button,
  CreateLotPage,
  CustomerDetailPage,
  CustomersManagementPage,
  LiveBidPanel,
  LotsManagementPage,
  OpsPageShell,
  Panel,
  SettlementRecoveryCard,
  StatusBadge,
  type OpsSection,
  type PublicBid,
  type RecoveryState,
} from '@gavel/ui';

const sectionPaths: Record<OpsSection, string> = {
  overview: '/admin',
  lots: '/admin/lots',
  customers: '/admin/customers',
  settlements: '/admin/settlements',
  workflows: '/admin/workflows',
};

const bids: PublicBid[] = [
  { id: 'b-184', alias: 'Silver Fox', resultingPrice: 42_800, occurredAt: '14:31:52', isMine: true },
  { id: 'b-183', alias: 'Atlas 17', resultingPrice: 42_600, occurredAt: '14:31:49' },
  { id: 'b-182', alias: 'North Star', resultingPrice: 42_100, occurredAt: '14:30:08' },
  { id: 'b-181', alias: 'Silver Fox', resultingPrice: 41_800, occurredAt: '14:29:44', isMine: true },
];

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
    window.scrollTo({ top: 0, behavior: 'smooth' });
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
    page = <LotWorkspace onNavigate={navigateSection} onBack={() => navigate('/admin/lots')} />;
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

  return <div className="min-h-screen bg-paper-100 p-0 md:p-3 xl:p-5">{page}</div>;
}

function LotWorkspace({ onNavigate, onBack }: { onNavigate: (section: OpsSection) => void; onBack: () => void }) {
  return <OpsPageShell section="lots" onNavigate={onNavigate} title="Omega Speedmaster, 1969" subtitle="GV-2841 · live auction" actions={<Button size="sm" variant="secondary" onClick={onBack}>Back to lots</Button>}>
    <div className="mb-5 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-moss-500/20 bg-moss-500/10 p-4">
      <div><p className="m-0 text-xs font-bold text-moss-600">Auction is live</p><p className="mt-1 mb-0 text-xs text-ink-500">184 accepted bids · 1,248 watching</p></div>
      <StatusBadge tone="success">08:42 left</StatusBadge>
    </div>
    <LiveBidPanel currentPrice={42_800} minimumBid={43_000} endsAt="08:42 left" bids={bids} initialMaximum={45_000} status="leading" />
  </OpsPageShell>;
}

function SettlementsPage({ onNavigate }: { onNavigate: (section: OpsSection) => void }) {
  const [state, setState] = useState<RecoveryState>('recovery_required');
  const [notice, setNotice] = useState('Recovery link prepared. No operator action recorded yet.');
  const transition = (next: RecoveryState, message: string) => { setState(next); setNotice(message); };
  return <OpsPageShell section="settlements" onNavigate={onNavigate} title="Settlement recovery" subtitle="7 cases require operator attention">
    <SettlementRecoveryCard state={state} caseId="STL-4912" amount="€42,800" closingPrice="€42,800" candidateAlias="Silver Fox" round={state === 'expired' ? 2 : 1} deadline="16:27 UTC" recoveryUrl="https://pay.gavel.test/recover/STL-4912?token=••••••••" onCopyLink={() => setNotice('Secure recovery link copied.')} onOpenRecovery={() => transition('processing', 'Customer opened recovery; waiting for the provider outcome.')} />
    <div role="status" className="mt-4 flex flex-col gap-3 rounded-xl border border-paper-200 bg-paper-50 p-4 sm:flex-row sm:items-center">
      <p className="m-0 flex-1 text-xs text-ink-700">{notice}</p>
      <div className="flex flex-wrap gap-2"><Button size="sm" variant="secondary" onClick={() => transition('recovery_required', 'Recovery window reopened for the current candidate.')}>Reset</Button><Button size="sm" variant="secondary" disabled={state === 'succeeded' || state === 'expired'} onClick={() => transition('expired', 'Recovery expired; workflow advanced to candidate 2.')}>Expire window</Button><Button size="sm" disabled={state === 'succeeded' || state === 'expired'} onClick={() => transition('succeeded', 'Payment succeeded; winner confirmed and notifications scheduled.')}>Confirm payment</Button></div>
    </div>
  </OpsPageShell>;
}

function WorkflowsPage({ onNavigate }: { onNavigate: (section: OpsSection) => void }) {
  const [step, setStep] = useState<'recovery' | 'payment' | 'complete'>('recovery');
  return <OpsPageShell section="workflows" onNavigate={onNavigate} title="Auction workflows" subtitle="Temporal-backed process visibility">
    <Panel title="Settlement workflow" eyebrow="Live execution" action={<StatusBadge tone={step === 'complete' ? 'success' : 'warning'}>{step === 'recovery' ? 'Waiting for customer' : step === 'payment' ? 'Authorizing' : 'Completed'}</StatusBadge>}><div className="p-4"><AuctionFlow /><div className="mt-4 flex flex-wrap items-center gap-2"><p className="mr-auto m-0 text-xs text-ink-500">Replay a safe operator view of the workflow state.</p><Button size="sm" variant="secondary" onClick={() => setStep('recovery')}>Recovery</Button><Button size="sm" variant="secondary" onClick={() => setStep('payment')}>Payment</Button><Button size="sm" onClick={() => setStep('complete')}>Complete</Button></div></div></Panel>
  </OpsPageShell>;
}

function LiveMonitorPage({ onNavigate, onOpenLot }: { onNavigate: (section: OpsSection) => void; onOpenLot: () => void }) {
  return <OpsPageShell section="overview" onNavigate={onNavigate} title="Live auction monitor" subtitle="128 auctions · 2,418 accepted bids/min" actions={<Button size="sm" onClick={onOpenLot}>Open hottest lot</Button>}>
    <div className="grid gap-4 sm:grid-cols-3"><MonitorMetric label="Command acceptance" value="99.99%" detail="p99 84 ms" /><MonitorMetric label="Realtime fan-out" value="128 ms" detail="p99 · healthy" /><MonitorMetric label="Bid rejection" value="0.34%" detail="mostly below minimum" /></div>
    <Panel className="mt-5" title="Hottest auction" eyebrow="GV-2841"><div className="p-4"><LiveBidPanel currentPrice={42_800} minimumBid={43_000} endsAt="08:42 left" bids={bids} initialMaximum={45_000} status="leading" /></div></Panel>
  </OpsPageShell>;
}

function OperationsInboxPage({ onNavigate, navigate }: { onNavigate: (section: OpsSection) => void; navigate: (path: string) => void }) {
  const items = [
    ['Payment recovery', 'STL-4912', '01:42 left', '/admin/settlements', 'danger'],
    ['Identity review', 'USR-0834', 'High-value velocity', '/admin/customers/USR-0834', 'warning'],
    ['Deadline conflict', 'GV-2839', 'Soft-close ceiling', '/admin/lots/GV-2839', 'warning'],
    ['Image processing', 'LOT-D92F', 'Waiting 8 minutes', '/admin/lots/new', 'neutral'],
  ] as const;
  return <OpsPageShell section="overview" onNavigate={onNavigate} title="Operations inbox" subtitle="Prioritized exceptions across auction workflows">
    <Panel title="Needs attention" eyebrow="7 open cases"><div className="divide-y divide-paper-200">{items.map(([title, id, detail, path, tone]) => <button key={id} onClick={() => navigate(path)} className="grid w-full grid-cols-[10px_1fr_auto] items-center gap-4 p-4 text-left hover:bg-paper-100"><span className={`size-2.5 rounded-full ${tone === 'danger' ? 'bg-red-700' : tone === 'warning' ? 'bg-amber-600' : 'bg-ink-500'}`} /><span><strong className="block text-sm">{title}</strong><span className="mt-1 block font-mono text-[10px] text-ink-500">{id} · {detail}</span></span><span className="text-xs font-semibold text-copper-600">Review →</span></button>)}</div></Panel>
  </OpsPageShell>;
}

function ObservabilityPage({ onNavigate }: { onNavigate: (section: OpsSection) => void }) {
  return <OpsPageShell section="overview" onNavigate={onNavigate} title="Platform observability" subtitle="Critical paths · last 15 minutes">
    <div className="grid gap-4 lg:grid-cols-2"><HealthPanel title="Auction commands" value="99.99%" detail="84 ms p99 · 0.02% errors" good /><HealthPanel title="Realtime fan-out" value="99.97%" detail="128 ms p99 · 3 reconnect spikes" good /><HealthPanel title="Settlement worker" value="12 pollers" detail="38 ms schedule-to-start" good /><HealthPanel title="Payment adapter" value="Degraded" detail="2.1% recovery challenge rate" /></div>
  </OpsPageShell>;
}

function MonitorMetric({ label, value, detail }: { label: string; value: string; detail: string }) { return <div className="rounded-xl border border-paper-200 bg-paper-50 p-4 shadow-panel"><p className="m-0 text-[10px] font-bold uppercase tracking-wider text-ink-500">{label}</p><p className="mt-2 mb-0 text-2xl font-bold">{value}</p><p className="mt-1 mb-0 text-xs text-ink-500">{detail}</p></div>; }
function HealthPanel({ title, value, detail, good }: { title: string; value: string; detail: string; good?: boolean }) { return <Panel title={title} eyebrow="Service level" action={<StatusBadge tone={good ? 'success' : 'warning'}>{good ? 'Healthy' : 'Degraded'}</StatusBadge>}><div className="p-5"><p className="m-0 text-3xl font-bold tracking-tight">{value}</p><p className="mt-2 mb-0 text-xs text-ink-500">{detail}</p><div className="mt-5 h-16 rounded-lg bg-[linear-gradient(135deg,transparent_0_8%,rgba(189,86,52,.14)_8%_10%,transparent_10%_18%,rgba(189,86,52,.2)_18%_20%,transparent_20%_28%,rgba(46,118,98,.2)_28%_31%,transparent_31%)]" /></div></Panel>; }
