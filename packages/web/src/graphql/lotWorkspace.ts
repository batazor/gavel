import { configuredGraphQLEndpoint, configuredGraphQLSubscriptionsEndpoint, executeGraphQL, subscribeGraphQL } from './client';

export const LOT_WORKSPACE_OPERATION = /* GraphQL */ `
  query LotWorkspace($lotId: ID!, $historyFirst: Int! = 20) {
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
        bidHistory(first: $historyFirst) {
          asOfVersion
          edges {
            cursor
            node { bidderAlias resultingPrice currency version acceptedAt }
          }
        }
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

export const AUCTION_UPDATES_OPERATION = /* GraphQL */ `
  subscription AuctionUpdates($auctionId: ID!, $afterVersion: Int) {
    auctionUpdates(auctionId: $auctionId, afterVersion: $afterVersion) {
      auctionId
      version
      kind
      currentPrice
      currency
      endsAt
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
      bidHistory: {
        asOfVersion: number;
        edges: Array<{ cursor: string; node: { bidderAlias: string; resultingPrice: string; currency: string; version: number; acceptedAt: string } }>;
      };
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

export type AuctionUpdate = {
  auctionId: string;
  version: number;
  kind: 'PRICE_CHANGED' | 'DEADLINE_CHANGED' | 'CLOSED' | 'PAID' | 'CANCELLED';
  currentPrice: number | null;
  currency: string | null;
  endsAt: string | null;
};

const fixture: LotWorkspaceModel = {
  id: 'GV-2841',
  title: 'Omega Speedmaster, 1969',
  description: 'Calibre 861, original stepped dial, recently serviced.',
  source: 'fixture',
  auction: { id: 'AUC-2841', status: 'OPEN', endsAt: '08:42 left', currentPrice: 42_800, currency: 'EUR', version: 184, minimumBid: 43_000, viewerStatus: 'leading' },
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
      version: Math.max(auction.currentPrice.version, auction.bidHistory.asOfVersion),
      minimumBid: currentPrice + 200,
      viewerStatus,
    },
    bids: auction.bidHistory.edges.map(({ cursor, node }) => ({
      id: cursor,
      alias: node.bidderAlias,
      resultingPrice: amount(node.resultingPrice, 'bidHistory.resultingPrice'),
      occurredAt: node.acceptedAt,
    })),
  };
}

export async function getLotWorkspace(lotId: string, signal?: AbortSignal): Promise<LotWorkspaceModel> {
  const endpoint = configuredGraphQLEndpoint();
  if (!endpoint) {
    const auction = fixtureAuctions.get(fixture.auction.id) ?? { ...fixture.auction };
    fixtureAuctions.set(auction.id, auction);
    return { ...fixture, id: lotId, auction: { ...auction }, bids: [...fixture.bids] };
  }
  const response = await executeGraphQL<LotWorkspaceResponse, { lotId: string; historyFirst: number }>(endpoint, 'LotWorkspace', LOT_WORKSPACE_OPERATION, { lotId, historyFirst: 20 }, signal);
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
  if (!endpoint) {
    await new Promise<void>((resolve, reject) => {
      const timeout = window.setTimeout(resolve, 350);
      signal?.addEventListener('abort', () => { window.clearTimeout(timeout); reject(new DOMException('Aborted', 'AbortError')); }, { once: true });
    });
    const previous = fixtureAuctions.get(auctionId) ?? fixture.auction;
    const next = { ...previous, currentPrice: Math.min(maximumAmount, previous.minimumBid), minimumBid: Math.min(maximumAmount, previous.minimumBid) + 200, version: previous.version + 1, viewerStatus: 'leading' as const };
    fixtureAuctions.set(auctionId, next);
    return { acceptedVersion: next.version, outbid: false, currentPrice: next.currentPrice, currency: next.currency, endsAt: next.endsAt, status: next.status, viewerStatus: next.viewerStatus };
  }

  const response = await executeGraphQL<PlaceBidResponse, { input: { auctionId: string; maximumAmount: string; currency: string }; idempotencyKey: string }>(
    endpoint,
    'PlaceBid',
    PLACE_BID_OPERATION,
    { input: { auctionId, maximumAmount: String(maximumAmount), currency }, idempotencyKey: crypto.randomUUID() },
    signal,
  );
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

type AuctionUpdatesResponse = {
  auctionUpdates: {
    auctionId: string;
    version: number;
    kind: AuctionUpdate['kind'];
    currentPrice: string | null;
    currency: string | null;
    endsAt: string | null;
  };
};

export async function subscribeAuctionUpdates(auctionId: string, afterVersion: number, onUpdate: (update: AuctionUpdate) => void, signal: AbortSignal) {
  const endpoint = configuredGraphQLSubscriptionsEndpoint();
  if (!endpoint) return;
  await subscribeGraphQL<AuctionUpdatesResponse, { auctionId: string; afterVersion: number }>(endpoint, 'AuctionUpdates', AUCTION_UPDATES_OPERATION, { auctionId, afterVersion }, (response) => {
    const update = response.auctionUpdates;
    onUpdate({ ...update, currentPrice: update.currentPrice === null ? null : amount(update.currentPrice, 'auctionUpdates.currentPrice') });
  }, signal);
}
