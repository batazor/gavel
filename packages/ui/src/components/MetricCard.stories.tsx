import type { Meta, StoryObj } from '@storybook/react-vite';
import { Activity } from 'lucide-react';
import { MetricCard } from './MetricCard';

const meta = {
  title: 'Components/Metric card',
  component: MetricCard,
  args: { label: 'Bids / minute', value: '2,418', hint: 'vs yesterday', change: 12.1, icon: <Activity size={18} /> },
  decorators: [(Story) => <div className="w-[320px]"><Story /></div>],
} satisfies Meta<typeof MetricCard>;

export default meta;
type Story = StoryObj<typeof meta>;
export const Positive: Story = {};
export const Negative: Story = { args: { change: -4.8, label: 'Failed payments', value: '0.42%' } };
