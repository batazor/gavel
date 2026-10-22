import type { Meta, StoryObj } from '@storybook/react-vite';
import { AuctionFlow } from './AuctionFlow';

const meta = {
  title: 'Visualization/Auction flow',
  component: AuctionFlow,
  parameters: { layout: 'fullscreen' },
  decorators: [(Story) => <div className="bg-paper-100 p-8"><Story /></div>],
} satisfies Meta<typeof AuctionFlow>;

export default meta;
type Story = StoryObj<typeof meta>;
export const SettlementWorkflow: Story = {};
