import type { Meta, StoryObj } from '@storybook/react-vite';
import { Gavel, LayoutDashboard, Workflow } from 'lucide-react';
import { AdminShell } from './AdminShell';
import { Button } from './Button';
import { Panel } from './Panel';

const meta = {
  title: 'Patterns/Admin shell',
  component: AdminShell,
  parameters: { layout: 'fullscreen' },
} satisfies Meta<typeof AdminShell>;

export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {
  args: {
    title: 'Auction operations',
    subtitle: 'Live system pulse',
    nav: [{ label: 'Overview', icon: <LayoutDashboard size={16} />, active: true }, { label: 'Auctions', icon: <Gavel size={16} />, badge: '12' }, { label: 'Workflows', icon: <Workflow size={16} /> }],
    actions: <Button size="sm">New auction</Button>,
    children: <Panel title="Workspace"><div className="p-8 text-sm text-ink-500">Admin content goes here.</div></Panel>,
  },
};
