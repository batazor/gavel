import { configuredGraphQLEndpoint, executeGraphQL, GraphQLRequestError } from './client';

export const LOT_WORKSPACE_OPERATION = /* GraphQL */ `
  query LotWorkspace($lotId: ID!) {
    lot(id: $lotId) {
      id
      title
      description
      images { id url alt }
      auction {
        id
        status
        endsAt
        currentPrice { amount currency version }
        viewer { isLeader isSettlementCandidate isWinner canBid }
      }
    }
  }
`;

export const PLACE_BID_OPERATION = /* GraphQL */ `
  mutation PlaceBid($input: PlaceBidInput!, $idempotencyKey: String!) {
    placeBid(input: $input, idempotencyKey: $idempotencyKey) {
      acceptedVersion
      outbid
      auction {
        id
        status
        endsAt
        currentPrice { amount currency version }
        viewer { isLeader isSettlementCandidate isWinner canBid }
      }
    }
  }
`;

type LotWorkspaceResponse = {
  lot: null | {
    id: string;
    title: string;
    description: string | null;
    images: Array<{ id: string; url: string; alt: string }>;
    auction: null | {
      id: string;
      status: 'SCHEDULED' | 'OPEN' | 'SETTLING' | 'SOLD' | 'UNSOLD' | 'CANCELLED';
      endsAt: string;
      currentPrice: { amount: string; currency: string; version: number };
      viewer: null | { isLeader: boolean; isSettlementCandidate: boolean; isWinner: boolean; canBid: boolean };
    };
  };
};

export type LotWorkspaceModel = {
  id: string;
  title: string;
  description: string | null;
  source: 'bff' | 'fixture';
  auction: {
    id: string;
    status: 'SCHEDULED' | 'OPEN' | 'SETTLING' | 'SOLD' | 'UNSOLD' | 'CANCELLED';
    endsAt: string;
    currentPrice: number;
    currency: string;
    version: number;
    minimumBid: number;
    viewerStatus: 'leading' | 'outbid' | 'watching';
    canBid: boolean;
    isSettlementCandidate: boolean;
    isWinner: boolean;
  };
  bids: Array<{ id: string; alias: string; resultingPrice: number; occurredAt: string; isMine?: boolean }>;
};

export type PlaceBidOutcome = {
  acceptedVersion: number;
  outbid: boolean;
  currentPrice: number;
  currency: string;
  endsAt: string;
  status: LotWorkspaceModel['auction']['status'];
  viewerStatus: LotWorkspaceModel['auction']['viewerStatus'];
};

export type BidErrorCode = 'AUCTION_CLOSED' | 'BID_TOO_LOW' | 'PAYMENT_NOT_READY' | 'FORBIDDEN' | 'CONFLICT' | 'NETWORK_ERROR' | 'UNKNOWN';

export class PlaceBidError extends Error {
  constructor(readonly code: BidErrorCode, message: string) {
    super(message);
    this.name = 'PlaceBidError';
  }
}

const fixture: LotWorkspaceModel = {
  id: 'GV-2841',
  title: 'Omega Speedmaster, 1969',
  description: 'Calibre 861, original stepped dial, recently serviced.',
  source: 'fixture',
  auction: { id: 'AUC-2841', status: 'OPEN', endsAt: '08:42 left', currentPrice: 42_800, currency: 'EUR', version: 184, minimumBid: 43_000, viewerStatus: 'leading', canBid: true, isSettlementCandidate: false, isWinner: false },
  bids: [
    { id: '184', alias: 'Silver Fox', resultingPrice: 42_800, occurredAt: '14:31:52', isMine: true },
    { id: '183', alias: 'Atlas 17', resultingPrice: 42_600, occurredAt: '14:31:49' },
    { id: '182', alias: 'North Star', resultingPrice: 42_100, occurredAt: '14:30:08' },
    { id: '181', alias: 'Silver Fox', resultingPrice: 41_800, occurredAt: '14:29:44', isMine: true },
  ],
};

const fixtureAuctions = new Map<string, LotWorkspaceModel['auction']>();

function amount(value: string, field: string) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) throw new Error(`Invalid money value for ${field}.`);
  return parsed;
}

function mapResponse(response: LotWorkspaceResponse): LotWorkspaceModel {
  const lot = response.lot;
  if (!lot) throw new Error('Lot not found.');
  if (!lot.auction) throw new Error('The lot does not have an auction yet.');
  const auction = lot.auction;
  const currentPrice = amount(auction.currentPrice.amount, 'currentPrice');
  const viewerStatus = auction.viewer?.isLeader ? 'leading' : auction.viewer?.canBid ? 'watching' : 'outbid';
  return {
    id: lot.id,
    title: lot.title,
    description: lot.description,
    source: 'bff',
    auction: {
      id: auction.id,
      status: auction.status,
      endsAt: auction.endsAt,
      currentPrice,
      currency: auction.currentPrice.currency,
      version: auction.currentPrice.version,
      minimumBid: currentPrice + 200,
      viewerStatus,
      canBid: auction.viewer?.canBid ?? false,
      isSettlementCandidate: auction.viewer?.isSettlementCandidate ?? false,
      isWinner: auction.viewer?.isWinner ?? false,
    },
    bids: [],
  };
}

export function isTerminalAuction(status: LotWorkspaceModel['auction']['status']) {
  return status === 'SOLD' || status === 'UNSOLD' || status === 'CANCELLED';
}

export function newerWorkspace(current: LotWorkspaceModel | null, incoming: LotWorkspaceModel) {
  if (!current || current.auction.id !== incoming.auction.id) return incoming;
  return incoming.auction.version > current.auction.version ? incoming : current;
}

function configuredMode() {
  const mode = import.meta.env.VITE_DATA_MODE?.trim() || 'demo';
  if (mode !== 'demo' && mode !== 'connected') throw new Error('VITE_DATA_MODE must be demo or connected.');
  return mode;
}

export async function getLotWorkspace(lotId: string, signal?: AbortSignal): Promise<LotWorkspaceModel> {
  const endpoint = configuredGraphQLEndpoint();
  if (configuredMode() === 'demo') {
    const auction = fixtureAuctions.get(fixture.auction.id) ?? { ...fixture.auction };
    fixtureAuctions.set(auction.id, auction);
    return { ...fixture, id: lotId, auction: { ...auction }, bids: [...fixture.bids] };
  }
  if (!endpoint) throw new Error('Connected mode requires VITE_GRAPHQL_URL.');
  const response = await executeGraphQL<LotWorkspaceResponse, { lotId: string }>(endpoint, 'LotWorkspace', LOT_WORKSPACE_OPERATION, { lotId }, signal);
  return mapResponse(response);
}

type PlaceBidResponse = {
  placeBid: {
    acceptedVersion: number;
    outbid: boolean;
    auction: {
      id: string;
      status: LotWorkspaceModel['auction']['status'];
      endsAt: string;
      currentPrice: { amount: string; currency: string; version: number };
      viewer: null | { isLeader: boolean; canBid: boolean };
    };
  };
};

export async function placeBid(auctionId: string, maximumAmount: number, currency: string, signal?: AbortSignal): Promise<PlaceBidOutcome> {
  const endpoint = configuredGraphQLEndpoint();
  if (configuredMode() === 'demo') {
    await new Promise<void>((resolve, reject) => {
      const timeout = window.setTimeout(resolve, 350);
      signal?.addEventListener('abort', () => { window.clearTimeout(timeout); reject(new DOMException('Aborted', 'AbortError')); }, { once: true });
    });
    const previous = fixtureAuctions.get(auctionId) ?? fixture.auction;
    const next = { ...previous, currentPrice: Math.min(maximumAmount, previous.minimumBid), minimumBid: Math.min(maximumAmount, previous.minimumBid) + 200, version: previous.version + 1, viewerStatus: 'leading' as const };
    fixtureAuctions.set(auctionId, next);
    return { acceptedVersion: next.version, outbid: false, currentPrice: next.currentPrice, currency: next.currency, endsAt: next.endsAt, status: next.status, viewerStatus: next.viewerStatus };
  }
  if (!endpoint) throw new PlaceBidError('NETWORK_ERROR', 'Connected mode requires VITE_GRAPHQL_URL.');

  let response: PlaceBidResponse;
  try {
    response = await executeGraphQL<PlaceBidResponse, { input: { auctionId: string; maximumAmount: string; currency: string }; idempotencyKey: string }>(
      endpoint,
      'PlaceBid',
      PLACE_BID_OPERATION,
      { input: { auctionId, maximumAmount: String(maximumAmount), currency }, idempotencyKey: crypto.randomUUID() },
      signal,
    );
  } catch (error) {
    if (!(error instanceof GraphQLRequestError)) throw error;
    const code = normalizeBidErrorCode(error.code);
    throw new PlaceBidError(code, bidErrorMessage(code));
  }
  const result = response.placeBid;
  const currentPrice = amount(result.auction.currentPrice.amount, 'placeBid.auction.currentPrice');
  return {
    acceptedVersion: Math.max(result.acceptedVersion, result.auction.currentPrice.version),
    outbid: result.outbid,
    currentPrice,
    currency: result.auction.currentPrice.currency,
    endsAt: result.auction.endsAt,
    status: result.auction.status,
    viewerStatus: result.auction.viewer?.isLeader ? 'leading' : result.auction.viewer?.canBid ? 'watching' : 'outbid',
  };
}

function normalizeBidErrorCode(code?: string): BidErrorCode {
  switch (code) {
    case 'AUCTION_CLOSED':
    case 'BID_TOO_LOW':
    case 'PAYMENT_NOT_READY':
    case 'FORBIDDEN':
    case 'CONFLICT':
    case 'NETWORK_ERROR':
      return code;
    default:
      return 'UNKNOWN';
  }
}

function bidErrorMessage(code: BidErrorCode) {
  switch (code) {
    case 'AUCTION_CLOSED': return 'This auction is no longer accepting bids.';
    case 'BID_TOO_LOW': return 'Your maximum is below the current minimum bid.';
    case 'PAYMENT_NOT_READY': return 'Add a ready payment instrument before bidding.';
    case 'FORBIDDEN': return 'This identity is not allowed to bid on the auction.';
    case 'CONFLICT': return 'The bid conflicts with a newer auction version. The lot will refresh.';
    case 'NETWORK_ERROR': return 'The bid service is unreachable. Your request can be retried safely.';
    default: return 'The bid could not be accepted.';
  }
}
