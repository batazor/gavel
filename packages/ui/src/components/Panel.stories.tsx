import type { Meta, StoryObj } from '@storybook/react-vite';
import { EmptyState, ErrorState, Panel, Skeleton } from './Panel';
import { Button } from './Button';

const meta = {
  title: 'Components/Panel',
  component: Panel,
  parameters: { layout: 'centered', docs: { description: { component: 'A near-white sheet with a 1px line and an 8px radius, no shadow. The title at 16/500, the meta in grey beside it, one action on the right. Every block has an empty, loading and error state that keeps the frame.' } } },
  decorators: [(Story) => <div className="w-[min(560px,95vw)]"><Story /></div>],
  args: { title: 'Needs an operator', meta: '7 open · 1 time-critical' },
} satisfies Meta<typeof Panel>;

export default meta;
type Story = StoryObj<typeof meta>;

export const WithRows: Story = {
  args: { action: <Button variant="secondary" size="sm">View all 7</Button> },
  render: (args) => (
    <Panel {...args}>
      <ul className="divide-y divide-rule border-t border-rule">
        {['Payment recovery · STL-4912', 'Identity review · USR-0834', 'Deadline conflict · GV-2839'].map((row) => <li key={row} className="flex h-11 items-center px-5 text-sm text-ink">{row}</li>)}
      </ul>
    </Panel>
  ),
};
export const Empty: Story = { args: { meta: '0 open' }, render: (args) => <Panel {...args}><EmptyState>Nothing is waiting on this desk right now.</EmptyState></Panel> };
export const Loading: Story = { render: (args) => <Panel {...args}><div className="grid gap-3 px-5 pb-5"><Skeleton height={14} width={260} /><Skeleton height={14} width={220} /><Skeleton height={14} width={240} /></div></Panel> };
export const Error: Story = { render: (args) => <Panel {...args}><ErrorState message="Couldn't load the queue." lastGood="14:19" onRetry={() => undefined} /></Panel> };
