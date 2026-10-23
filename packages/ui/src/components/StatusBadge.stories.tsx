import type { Meta, StoryObj } from '@storybook/react-vite';
import { StatusBadge } from './StatusBadge';

const meta = {
  title: 'Components/Status badge',
  component: StatusBadge,
  parameters: { docs: { description: { component: 'A pale tint with dark text of the same hue, 22px tall, sentence case. One per row; a dot only for a live or running state.' } } },
  args: { children: 'Live', tone: 'success', dot: true },
} satisfies Meta<typeof StatusBadge>;

export default meta;
type Story = StoryObj<typeof meta>;
export const Playground: Story = {};
export const StatusSet: Story = {
  render: () => <div className="flex flex-wrap gap-3"><StatusBadge>Draft</StatusBadge><StatusBadge tone="warning">Review</StatusBadge><StatusBadge tone="success" dot>Live</StatusBadge><StatusBadge tone="neutral">Closed</StatusBadge><StatusBadge tone="danger">Expired</StatusBadge></div>,
};
