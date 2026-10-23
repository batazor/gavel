import { useCallback, useEffect, useRef, useState } from 'react';
import { getLotWorkspace, placeBid as submitPlaceBid, subscribeAuctionUpdates, type LotWorkspaceModel } from './lotWorkspace';

export function useLotWorkspace(lotId: string) {
  const [data, setData] = useState<LotWorkspaceModel | null>(null);
  const [error, setError] = useState<Error | null>(null);
  const [loading, setLoading] = useState(true);
  const [attempt, setAttempt] = useState(0);
  const [bidPending, setBidPending] = useState(false);
  const [bidError, setBidError] = useState<Error | null>(null);
  const versionRef = useRef(0);

  useEffect(() => {
    const controller = new AbortController();
    setLoading(true);
    setError(null);
    getLotWorkspace(lotId, controller.signal).then((result) => {
      versionRef.current = result.auction.version;
      setData(result);
      setLoading(false);
    }).catch((reason: unknown) => {
      if (reason instanceof DOMException && reason.name === 'AbortError') return;
      setError(reason instanceof Error ? reason : new Error('Unknown BFF error.'));
      setLoading(false);
    });
    return () => controller.abort();
  }, [attempt, lotId]);

  const auctionId = data?.auction.id;
  const source = data?.source;
  useEffect(() => {
    if (!auctionId || source !== 'bff') return;
    const controller = new AbortController();
    let reconnect: number | undefined;

    const connect = () => {
      subscribeAuctionUpdates(auctionId, versionRef.current, (update) => {
        setData((current) => {
          if (!current || update.auctionId !== current.auction.id || update.version <= current.auction.version) return current;
          versionRef.current = update.version;
          const status = update.kind === 'CANCELLED' ? 'CANCELLED' : update.kind === 'PAID' ? 'SOLD' : update.kind === 'CLOSED' ? 'SETTLING' : current.auction.status;
          const currentPrice = update.currentPrice ?? current.auction.currentPrice;
          return {
            ...current,
            auction: {
              ...current.auction,
              status,
              version: update.version,
              currentPrice,
              currency: update.currency ?? current.auction.currency,
              endsAt: update.endsAt ?? current.auction.endsAt,
              minimumBid: update.currentPrice === null ? current.auction.minimumBid : currentPrice + 200,
            },
          };
        });
      }, controller.signal).then(() => {
        if (!controller.signal.aborted) reconnect = window.setTimeout(connect, 1_000);
      }).catch((reason: unknown) => {
        if (controller.signal.aborted || (reason instanceof DOMException && reason.name === 'AbortError')) return;
        reconnect = window.setTimeout(connect, 1_500);
      });
    };

    connect();
    return () => {
      controller.abort();
      if (reconnect !== undefined) window.clearTimeout(reconnect);
    };
  }, [auctionId, source]);

  const placeBid = useCallback(async (maximumAmount: number) => {
    if (!data) throw new Error('Lot workspace is not loaded.');
    setBidPending(true);
    setBidError(null);
    try {
      const outcome = await submitPlaceBid(data.auction.id, maximumAmount, data.auction.currency);
      versionRef.current = Math.max(versionRef.current, outcome.acceptedVersion);
      setData((current) => current && current.auction.id === data.auction.id ? {
        ...current,
        auction: {
          ...current.auction,
          status: outcome.status,
          currentPrice: outcome.currentPrice,
          currency: outcome.currency,
          endsAt: outcome.endsAt,
          version: outcome.acceptedVersion,
          minimumBid: outcome.currentPrice + 200,
          viewerStatus: outcome.outbid ? 'outbid' : outcome.viewerStatus,
        },
        bids: [{ id: `v-${outcome.acceptedVersion}`, alias: 'You', resultingPrice: outcome.currentPrice, occurredAt: new Date().toLocaleTimeString('en-GB'), isMine: true }, ...current.bids],
      } : current);
    } catch (reason) {
      const nextError = reason instanceof Error ? reason : new Error('The bid could not be placed.');
      setBidError(nextError);
      throw nextError;
    } finally {
      setBidPending(false);
    }
  }, [data]);

  const retry = useCallback(() => setAttempt((current) => current + 1), []);
  return { data, error, loading, retry, placeBid, bidPending, bidError };
}
