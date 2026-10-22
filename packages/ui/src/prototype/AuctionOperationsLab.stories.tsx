import type { Meta, StoryObj } from '@storybook/react-vite';
import { AuctionOperationsLab } from './AuctionOperationsLab';

const meta = {
  title: 'Prototype/Admin directions',
  component: AuctionOperationsLab,
  parameters: {
    layout: 'fullscreen',
    docs: { description: { component: 'Throwaway design lab. Use the floating arrows or ←/→. The selected direction is stored in `?variant=A|B|C`.' } },
  },
} satisfies Meta<typeof AuctionOperationsLab>;

export default meta;
type Story = StoryObj<typeof meta>;
export const DirectionLab: Story = {};
