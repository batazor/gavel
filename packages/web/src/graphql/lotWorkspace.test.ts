import { describe, expect, it } from 'vitest';
import { isTerminalAuction, newerWorkspace, type LotWorkspaceModel } from './lotWorkspace';

function workspace(version: number, status: LotWorkspaceModel['auction']['status'] = 'OPEN'): LotWorkspaceModel {
  return {
    id: 'lot-1',
    title: 'Test lot',
    description: null,
    source: 'bff',
    auction: {
      id: 'auction-1',
      status,
      endsAt: '2026-10-23T12:00:00Z',
      currentPrice: 1_000 + version,
      currency: 'EUR',
      version,
      minimumBid: 1_200 + version,
      viewerStatus: 'watching',
      canBid: status === 'OPEN',
      isSettlementCandidate: false,
      isWinner: false,
    },
    bids: [],
  };
}

describe('version-aware LotWorkspace polling', () => {
  it('accepts a newer snapshot and ignores reordered or duplicate snapshots', () => {
    const current = workspace(12);
    expect(newerWorkspace(current, workspace(13)).auction.version).toBe(13);
    expect(newerWorkspace(current, workspace(12))).toBe(current);
    expect(newerWorkspace(current, workspace(11))).toBe(current);
  });

  it('stops only on terminal auction states', () => {
    expect(isTerminalAuction('OPEN')).toBe(false);
    expect(isTerminalAuction('SETTLING')).toBe(false);
    expect(isTerminalAuction('SOLD')).toBe(true);
    expect(isTerminalAuction('UNSOLD')).toBe(true);
    expect(isTerminalAuction('CANCELLED')).toBe(true);
  });
});
