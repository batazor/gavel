import type { Meta, StoryObj } from '@storybook/react-vite';
import { LiveBidPanel, type PublicBid } from './LiveBidPanel';

export const sampleBids: PublicBid[] = [
  { id: 'b-184', alias: 'Silver Fox', resultingPrice: 42_800, occurredAt: '14:31:52', isMine: true },
  { id: 'b-183', alias: 'Atlas 17', resultingPrice: 42_600, occurredAt: '14:31:49' },
  { id: 'b-182', alias: 'North Star', resultingPrice: 42_100, occurredAt: '14:30:08' },
  { id: 'b-181', alias: 'Silver Fox', resultingPrice: 41_800, occurredAt: '14:29:44', isMine: true },
  { id: 'b-180', alias: 'Mistral 4', resultingPrice: 41_500, occurredAt: '14:28:16' },
];

const meta = {
  title: 'Domain/Live bid panel',
  component: LiveBidPanel,
  parameters: { layout: 'centered' },
  decorators: [(Story) => <div className="w-[min(920px,95vw)]"><Story /></div>],
  args: { currentPrice: 42_800, minimumBid: 43_000, endsAt: '08:42 left', bids: sampleBids, initialMaximum: 45_000 },
} satisfies Meta<typeof LiveBidPanel>;

export default meta;
type Story = StoryObj<typeof meta>;
export const Watching: Story = {};
export const Leading: Story = { args: { status: 'leading' } };
export const Outbid: Story = { args: { status: 'outbid', initialMaximum: 43_000 } };
