import type { Meta, StoryObj } from '@storybook/react-vite';
import { SettlementRecoveryCard } from './SettlementRecoveryCard';

const meta = {
  title: 'Domain/Settlement recovery',
  component: SettlementRecoveryCard,
  parameters: { layout: 'centered' },
  decorators: [(Story) => <div className="w-[min(980px,95vw)]"><Story /></div>],
  args: { state: 'recovery_required', caseId: 'STL-4912', amount: '€42,800', closingPrice: '€42,800', candidateAlias: 'Silver Fox', round: 1, deadline: '16:27 UTC', timeLeft: '1 h 55 m', recoveryUrl: 'https://pay.gavel.test/recover/STL-4912?token=••••••••' },
} satisfies Meta<typeof SettlementRecoveryCard>;

export default meta;
type Story = StoryObj<typeof meta>;
export const RecoveryRequired: Story = {};
export const Processing: Story = { args: { state: 'processing' } };
export const Succeeded: Story = { args: { state: 'succeeded' } };
export const Expired: Story = { args: { state: 'expired', amount: '€31,200', round: 2 } };
