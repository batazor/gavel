import type { Meta, StoryObj } from '@storybook/react-vite';
import { Avatar } from './Avatar';
import { customers } from '../data/customers';

const meta = {
  title: 'Components/Avatar',
  component: Avatar,
  parameters: { layout: 'centered', docs: { description: { component: 'A stable initial on a pale tint of the person\'s own hue. Identity, not data: pale, and never in a chart legend. People listed together sit at least 60° apart.' } } },
  args: { name: 'Marta Weiss', hue: 20 },
} satisfies Meta<typeof Avatar>;

export default meta;
type Story = StoryObj<typeof meta>;
export const Default: Story = {};
export const Sizes: Story = { render: () => <div className="flex items-center gap-3"><Avatar name="Marta Weiss" hue={20} size="sm" /><Avatar name="Marta Weiss" hue={20} /><Avatar name="Marta Weiss" hue={20} size="lg" /></div> };
export const ADesk: Story = { render: () => <div className="flex items-center gap-3">{customers.map((c) => <Avatar key={c.id} name={c.name} hue={c.hue} />)}</div> };
