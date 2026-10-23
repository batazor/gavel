import type { Meta, StoryObj } from '@storybook/react-vite';
import { ClosingBoard } from './ClosingBoard';
import { Panel } from './Panel';
import { closingSoon, today } from '../data/ops';

const meta = {
  title: 'Domain/Closing board',
  component: ClosingBoard,
  parameters: { layout: 'centered', docs: { description: { component: 'The auction house\'s own element: every lot closing in the next hour on one strip. The scheduled hammer is an ink dot, a soft-close extension already granted is a copper segment after it, and now is a 1px copper line.' } } },
  decorators: [(Story) => <div className="w-[min(760px,95vw)]"><Story /></div>],
  args: { now: today.nowMinutes, rows: closingSoon.map((lot) => ({ id: lot.id, title: lot.title, price: lot.currentPrice, scheduledEnd: lot.scheduledEnd!, extendedTo: lot.extendedTo })) },
} satisfies Meta<typeof ClosingBoard>;

export default meta;
type Story = StoryObj<typeof meta>;
export const NextHour: Story = {};
export const InAPanel: Story = { render: (args) => <Panel title="Closing in the next hour" meta={`${args.rows.length} lots`}><div className="px-5 pb-4"><ClosingBoard {...args} /></div></Panel> };
export const OneLot: Story = { args: { rows: [{ id: 'GV-2841', title: 'Omega Speedmaster, 1969', price: 42_800, scheduledEnd: today.nowMinutes + 8, extendedTo: today.nowMinutes + 10 }] } };
