import type { Meta, StoryObj } from '@storybook/react-vite';
import { ClosingPolicyBuilder } from './ClosingPolicyBuilder';

const meta = {
  title: 'Domain/Closing policy builder',
  component: ClosingPolicyBuilder,
  parameters: { layout: 'centered' },
  decorators: [(Story) => <div className="w-[min(620px,95vw)]"><Story /></div>],
  args: { initialValue: { scheduledEnd: '2026-10-24T18:00', inactivityMinutes: 5, hardDeadline: '2026-10-24T20:00' } },
} satisfies Meta<typeof ClosingPolicyBuilder>;

export default meta;
type Story = StoryObj<typeof meta>;
export const AntiSniping: Story = {};
export const InactivityOnly: Story = { args: { initialValue: { scheduledEnd: null, inactivityMinutes: 10, hardDeadline: null } } };
export const LockedAfterFirstBid: Story = { args: { hasBids: true } };
