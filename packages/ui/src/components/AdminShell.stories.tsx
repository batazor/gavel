import type { Meta, StoryObj } from '@storybook/react-vite';
import { CircleDollarSign, LayoutDashboard, PackageSearch, Users, Workflow } from 'lucide-react';
import { AdminShell } from './AdminShell';
import { Button } from './Button';
import { Panel, EmptyState } from './Panel';

const meta = {
  title: 'Patterns/Admin shell',
  component: AdminShell,
  parameters: { layout: 'fullscreen' },
  decorators: [(Story) => <div className="h-dvh"><Story /></div>],
} satisfies Meta<typeof AdminShell>;

export default meta;
type Story = StoryObj<typeof meta>;

const nav = [
  { label: 'Overview', icon: <LayoutDashboard />, active: true },
  { label: 'Lots', icon: <PackageSearch /> },
  { label: 'Customers', icon: <Users /> },
  { label: 'Settlements', icon: <CircleDollarSign />, badge: '2' },
  { label: 'Workflows', icon: <Workflow /> },
];

export const OpenPlane: Story = {
  args: {
    title: 'Overview',
    subtitle: 'Payment recovery STL-4912 expires at 16:27, 1 h 55 m from now. 1 of 7 open cases is time-critical.',
    nav,
    actions: <><Button variant="secondary">Live monitor</Button><Button>Open inbox 7</Button></>,
    children: <Panel title="Workspace" meta="empty state"><EmptyState>Nothing is waiting on this desk right now.</EmptyState></Panel>,
  },
};

export const NoActions: Story = {
  args: { title: 'Workflows', subtitle: 'Temporal executions behind the auctions', nav: nav.map((item) => ({ ...item, active: item.label === 'Workflows' })), children: <Panel title="Settlement workflow" meta="GV-2841 · running"><EmptyState>No workflow selected.</EmptyState></Panel> },
};
