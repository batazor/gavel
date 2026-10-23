import type { ReactNode } from 'react';
import { CircleDollarSign, LayoutDashboard, PackageSearch, Users, Workflow } from 'lucide-react';
import { AdminShell } from '../components/AdminShell';
import { queue, today, operator } from '../data/ops';

export type OpsSection = 'overview' | 'lots' | 'customers' | 'settlements' | 'workflows';

const settlementsWaiting = queue.filter((item) => item.target === 'settlements').length;

export function OpsPageShell({ section, title, subtitle, actions, children, onNavigate }: { section: OpsSection; title: string; subtitle?: ReactNode; actions?: ReactNode; children: ReactNode; onNavigate?: (section: OpsSection) => void }) {
  const items: Array<{ key: OpsSection; label: string; icon: ReactNode; badge?: string }> = [
    { key: 'overview', label: 'Overview', icon: <LayoutDashboard /> },
    { key: 'lots', label: 'Lots', icon: <PackageSearch /> },
    { key: 'customers', label: 'Customers', icon: <Users /> },
    { key: 'settlements', label: 'Settlements', icon: <CircleDollarSign />, badge: String(settlementsWaiting) },
    { key: 'workflows', label: 'Workflows', icon: <Workflow /> },
  ];
  const searchItems = [
    { label: 'Omega Speedmaster, 1969', meta: 'GV-2841 · live', group: 'Lots', section: 'lots' as const },
    { label: 'Untitled lithograph, 1974', meta: 'GV-2817 · in review', group: 'Lots', section: 'lots' as const },
    { label: 'Marta Weiss', meta: 'USR-0834 · identity review', group: 'Customers', section: 'customers' as const },
    { label: 'Payment recovery', meta: 'STL-4912 · expires 16:27', group: 'Settlements', section: 'settlements' as const },
    { label: 'Settlement workflow', meta: 'Temporal · running', group: 'Workflows', section: 'workflows' as const },
  ];
  return (
    <AdminShell
      title={title}
      subtitle={subtitle}
      actions={actions}
      scope={`Operations · ${today.desk}`}
      user={operator}
      nav={items.map((item) => ({ ...item, active: item.key === section, onClick: onNavigate ? () => onNavigate(item.key) : undefined }))}
      searchItems={searchItems.map((item) => ({ label: item.label, meta: item.meta, group: item.group, onSelect: onNavigate ? () => onNavigate(item.section) : undefined }))}
    >
      {children}
    </AdminShell>
  );
}
