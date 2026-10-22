import type { Meta, StoryObj } from '@storybook/react-vite';
import { Plus } from 'lucide-react';
import { Button } from './Button';

const meta = {
  title: 'Components/Button',
  component: Button,
  parameters: { layout: 'centered' },
  args: { children: 'Create auction' },
} satisfies Meta<typeof Button>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {};
export const Secondary: Story = { args: { variant: 'secondary' } };
export const WithIcon: Story = { args: { leadingIcon: <Plus />, children: 'Add lot' } };
export const Danger: Story = { args: { variant: 'danger', children: 'Cancel auction' } };
export const Disabled: Story = { args: { disabled: true } };

export const AllVariants: Story = {
  render: () => <div className="flex flex-wrap items-center gap-3"><Button>Primary</Button><Button variant="secondary">Secondary</Button><Button variant="ghost">Ghost</Button><Button variant="danger">Danger</Button></div>,
};
