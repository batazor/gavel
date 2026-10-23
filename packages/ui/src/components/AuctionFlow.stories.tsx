import type { Meta, StoryObj } from '@storybook/react-vite';
import { AuctionFlow } from './AuctionFlow';

const meta = {
  title: 'Visualization/Auction flow',
  component: AuctionFlow,
  parameters: { layout: 'fullscreen', docs: { description: { component: 'A builder fragment: node-type glyphs sit in neutral tiles (allowed in a builder), the active node and edge carry the one hue.' } } },
  decorators: [(Story) => <div className="bg-ground p-8"><Story /></div>],
} satisfies Meta<typeof AuctionFlow>;

export default meta;
type Story = StoryObj<typeof meta>;
export const SettlementWorkflow: Story = {};
