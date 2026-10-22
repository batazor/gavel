import type { ReactNode } from 'react';
import { CircleDollarSign, LayoutDashboard, PackageSearch, Users, Workflow } from 'lucide-react';
import { AdminShell } from '../components/AdminShell';

export type OpsSection = 'overview' | 'lots' | 'customers' | 'settlements' | 'workflows';

export function OpsPageShell({ section, title, subtitle, actions, children, onNavigate }: { section: OpsSection; title: string; subtitle: string; actions?: ReactNode; children: ReactNode; onNavigate?: (section: OpsSection) => void }) {
  const items = [
    { key: 'overview', label: 'Overview', icon: <LayoutDashboard size={16} /> },
    { key: 'lots', label: 'Lots & auctions', icon: <PackageSearch size={16} />, badge: '12' },
    { key: 'customers', label: 'Customers', icon: <Users size={16} /> },
    { key: 'settlements', label: 'Settlements', icon: <CircleDollarSign size={16} />, badge: '7' },
    { key: 'workflows', label: 'Workflows', icon: <Workflow size={16} /> },
  ];
  return <AdminShell title={title} subtitle={subtitle} actions={actions} nav={items.map((item) => ({ ...item, active: item.key === section, onClick: onNavigate ? () => onNavigate(item.key as OpsSection) : undefined }))}>{children}</AdminShell>;
}

export function PageEyebrow({ children }: { children: ReactNode }) {
  return <p className="m-0 text-[10px] font-bold uppercase tracking-[0.15em] text-copper-600">{children}</p>;
}
