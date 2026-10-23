import type { Meta, StoryObj } from '@storybook/react-vite';
import { Plus } from 'lucide-react';
import { Button } from './Button';

const meta = {
  title: 'Components/Button',
  component: Button,
  parameters: { layout: 'centered', docs: { description: { component: 'One filled near-black primary per screen. The secondary is white with a 1px control border, the ghost is text, and danger stays outlined until pressed. Heights 28, 32 and 36; radius 6.' } } },
  args: { children: 'Create lot' },
} satisfies Meta<typeof Button>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {};
export const Secondary: Story = { args: { variant: 'secondary' } };
export const WithIcon: Story = { args: { leadingIcon: <Plus />, children: 'Add lot' } };
export const Danger: Story = { args: { variant: 'danger', children: 'Restrict member' } };
export const Disabled: Story = { args: { disabled: true } };

export const AllVariants: Story = {
  render: () => (
    <div className="grid gap-4">
      {(['sm', 'md', 'lg'] as const).map((size) => (
        <div key={size} className="flex flex-wrap items-center gap-3">
          <Button size={size}>Primary</Button>
          <Button size={size} variant="secondary">Secondary</Button>
          <Button size={size} variant="ghost">Ghost</Button>
          <Button size={size} variant="danger">Danger</Button>
          <Button size={size} disabled>Disabled</Button>
        </div>
      ))}
    </div>
  ),
};
