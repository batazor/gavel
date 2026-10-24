import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { LiveBidPanel } from '@gavel/ui';
import { terminalBidReason } from './App';

afterEach(cleanup);

describe('LotWorkspace bidding states', () => {
  it('renders a disabled terminal state instead of submitting another bid', () => {
    const placeBid = vi.fn();
    render(
      <LiveBidPanel
        currentPrice={1_000}
        minimumBid={1_200}
        endsAt="closed"
        bids={[]}
        biddingDisabled
        biddingDisabledReason={terminalBidReason('SOLD', true, false)}
        onPlaceBid={placeBid}
      />,
    );
    expect((screen.getByRole('button', { name: 'Place bid' }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText('Sold to you. The result is final.')).toBeTruthy();
    expect(placeBid).not.toHaveBeenCalled();
  });

  it('shows the typed mutation message returned by the client boundary', async () => {
    render(
      <LiveBidPanel
        currentPrice={1_000}
        minimumBid={1_200}
        endsAt="2 minutes"
        bids={[]}
        onPlaceBid={() => Promise.reject(new Error('Add a ready payment instrument before bidding.'))}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Place bid' }));
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('ready payment instrument'));
  });
});
