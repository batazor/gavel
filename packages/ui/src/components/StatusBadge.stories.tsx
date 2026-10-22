import type { Meta, StoryObj } from '@storybook/react-vite';
import { StatusBadge } from './StatusBadge';

const meta = {
  title: 'Components/Status badge',
  component: StatusBadge,
  args: { children: 'Live' },
} satisfies Meta<typeof StatusBadge>;

export default meta;
type Story = StoryObj<typeof meta>;
export const Playground: Story = {};
export const StatusSet: Story = {
  render: () => <div className="flex flex-wrap gap-3"><StatusBadge>Draft</StatusBadge><StatusBadge tone="info">Review</StatusBadge><StatusBadge tone="success">Live</StatusBadge><StatusBadge tone="warning">Ending</StatusBadge><StatusBadge tone="danger">Failed</StatusBadge></div>,
};
