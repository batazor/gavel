import type { Meta, StoryObj } from '@storybook/react-vite';
import { LiveBidPanel } from './LiveBidPanel';
import { speedmasterBids } from '../data/ops';

const meta = {
  title: 'Domain/Live bid panel',
  component: LiveBidPanel,
  parameters: { layout: 'centered', docs: { description: { component: 'The bidder\'s panel: the current price is the main data element and carries the one hue; one primary (Place bid); aliases get a pale identity tint.' } } },
  decorators: [(Story) => <div className="w-[min(920px,95vw)]"><Story /></div>],
  args: { currentPrice: 42_800, minimumBid: 43_000, endsAt: '8 m 42 s', bids: speedmasterBids, totalBids: 184, initialMaximum: 45_000 },
} satisfies Meta<typeof LiveBidPanel>;

export default meta;
type Story = StoryObj<typeof meta>;
export const Watching: Story = {};
export const Leading: Story = { args: { status: 'leading' } };
export const Outbid: Story = { args: { status: 'outbid', initialMaximum: 43_000 } };
export const Phone: Story = { args: { status: 'leading' }, parameters: { viewport: { defaultViewport: 'mobile1' } }, decorators: [(Story) => <div className="w-[358px]"><Story /></div>] };
export const RejectedBid: Story = { args: { status: 'outbid', onPlaceBid: async () => { throw new Error('Bid rejected: below the minimum after a concurrent bid.'); } } };
