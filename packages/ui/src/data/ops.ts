/*
  Sample operations data for the Gavel back office. Every figure the screens show comes from here,
  totals are computed from rows, and figures that appear twice are one variable.
  Replace with the BFF projection before launch (see README).
*/

export const today = {
  date: 'Thursday 22 October 2026',
  nowMinutes: 14 * 60 + 32,
  nowLabel: '14:32 UTC',
  desk: 'Berlin desk',
};

export type LotState = 'Live' | 'Review' | 'Draft' | 'Closed' | 'Withdrawn';
export type LotCover = 'watch' | 'chair' | 'camera' | 'print' | 'cabinet' | 'bag';

export interface Lot {
  id: string;
  title: string;
  seller: string;
  category: string;
  cover: LotCover;
  startingPrice: number;
  currentPrice: number;
  bids: number;
  watching: number;
  state: LotState;
  /** Scheduled end today, minutes since midnight UTC; null when it closes on another day or is not scheduled. */
  scheduledEnd: number | null;
  /** The soft-close extension already granted today, minutes since midnight. */
  extendedTo: number | null;
  /** When the lot does not close today. */
  closesOn: string | null;
}

export const lots: Lot[] = [
  { id: 'GV-2841', title: 'Omega Speedmaster, 1969', seller: 'Elena Marlowe', category: 'Watches', cover: 'watch', startingPrice: 18_000, currentPrice: 42_800, bids: 184, watching: 1_248, state: 'Live', scheduledEnd: 14 * 60 + 40, extendedTo: 14 * 60 + 42, closesOn: null },
  { id: 'GV-2831', title: 'Hermès Kelly 32, 1998', seller: 'Maison Keller', category: 'Fashion', cover: 'bag', startingPrice: 9_000, currentPrice: 15_200, bids: 92, watching: 604, state: 'Live', scheduledEnd: 15 * 60 + 5, extendedTo: null, closesOn: null },
  { id: 'GV-2839', title: 'Eames lounge chair', seller: 'Studio Nord', category: 'Design', cover: 'chair', startingPrice: 4_200, currentPrice: 8_240, bids: 47, watching: 386, state: 'Live', scheduledEnd: 14 * 60 + 50, extendedTo: null, closesOn: null },
  { id: 'GV-2836', title: 'Leica M6 Titanium', seller: 'Haruto Klein', category: 'Cameras', cover: 'camera', startingPrice: 6_000, currentPrice: 11_400, bids: 63, watching: 719, state: 'Live', scheduledEnd: null, extendedTo: null, closesOn: 'Fri 23 Oct, 18:00' },
  { id: 'GV-2817', title: 'Untitled lithograph, 1974', seller: 'Atelier 17', category: 'Art', cover: 'print', startingPrice: 16_900, currentPrice: 16_900, bids: 0, watching: 0, state: 'Review', scheduledEnd: null, extendedTo: null, closesOn: null },
  { id: 'GV-2802', title: 'Børge Mogensen cabinet', seller: 'Studio Nord', category: 'Design', cover: 'cabinet', startingPrice: 6_800, currentPrice: 6_800, bids: 0, watching: 0, state: 'Draft', scheduledEnd: null, extendedTo: null, closesOn: null },
  { id: 'GV-2788', title: 'Cartier Tank Louis, 1982', seller: 'Maison Keller', category: 'Watches', cover: 'watch', startingPrice: 5_000, currentPrice: 9_600, bids: 81, watching: 940, state: 'Closed', scheduledEnd: null, extendedTo: null, closesOn: 'Closed Wed 21 Oct' },
];

export const liveLots = lots.filter((lot) => lot.state === 'Live');
export const hotLots = [...liveLots].sort((a, b) => b.bids - a.bids);
export const closingSoon = liveLots.filter((lot) => lot.scheduledEnd !== null).sort((a, b) => (a.extendedTo ?? a.scheduledEnd!) - (b.extendedTo ?? b.scheduledEnd!));

export type QueueTone = 'danger' | 'warning' | 'neutral';
export type QueueTarget = 'settlements' | 'customers' | 'lots' | 'create';

export interface QueueItem {
  id: string;
  kind: string;
  object: string;
  detail: string;
  /** Minutes since midnight when the item expires, or null. */
  dueAt: number | null;
  tone: QueueTone;
  target: QueueTarget;
}

export const queue: QueueItem[] = [
  { id: 'STL-4912', kind: 'Payment recovery', object: 'Omega Speedmaster, 1969', detail: '€42,800 · candidate Silver Fox, round 1', dueAt: 16 * 60 + 27, tone: 'danger', target: 'settlements' },
  { id: 'USR-0834', kind: 'Identity review', object: 'Marta Weiss', detail: 'Bid velocity tripled today', dueAt: null, tone: 'warning', target: 'customers' },
  { id: 'GV-2839', kind: 'Deadline conflict', object: 'Eames lounge chair', detail: 'Hard deadline 15:00 is before the soft-close ceiling', dueAt: 15 * 60, tone: 'warning', target: 'lots' },
  { id: 'LOT-D92F', kind: 'Image processing', object: 'Draft lot, 3 images', detail: 'Waiting 8 min for the media pipeline', dueAt: null, tone: 'neutral', target: 'create' },
  { id: 'GV-2788', kind: 'Seller dispute', object: 'Cartier Tank Louis, 1982', detail: 'Buyer questions the condition report', dueAt: null, tone: 'neutral', target: 'lots' },
  { id: 'STL-4907', kind: 'Payment recovery', object: 'Nakashima side table', detail: '€8,100 · candidate Mistral 4, round 2', dueAt: 19 * 60 + 10, tone: 'neutral', target: 'settlements' },
  { id: 'GV-2817', kind: 'Lot review', object: 'Untitled lithograph, 1974', detail: 'Catalogue check before it can go live', dueAt: null, tone: 'neutral', target: 'lots' },
];

export const queueTotal = queue.length;
export const queueTimeCritical = queue.filter((item) => item.dueAt !== null && item.dueAt - today.nowMinutes <= 120).length;

/** Accepted bids per minute, one value per minute, from 13:32 to 14:32 (61 points). */
export const bidsPerMinute = [
  1_842, 1_866, 1_901, 1_878, 1_912, 1_940, 1_957, 1_923, 1_969, 1_998,
  2_014, 1_986, 2_031, 2_052, 2_040, 2_071, 2_095, 2_063, 2_108, 2_124,
  2_102, 2_139, 2_157, 2_131, 2_168, 2_190, 2_174, 2_203, 2_226, 2_211,
  2_244, 2_260, 2_238, 2_271, 2_289, 2_266, 2_297, 2_312, 2_284, 2_320,
  2_336, 2_309, 2_343, 2_361, 2_330, 2_374, 2_392, 2_366, 2_401, 2_388,
  2_410, 2_377, 2_604, 2_935, 3_112, 2_880, 2_612, 2_468, 2_431, 2_402,
  2_418,
];
export const bidsWindowStart = 13 * 60 + 32;
export const bidsLabels = bidsPerMinute.map((_, index) => bidsWindowStart + index);
export const bidsNow = bidsPerMinute[bidsPerMinute.length - 1];
export const bidsYesterday = 2_232;
export const bidsSpike = { at: 54, label: 'GV-2841 soft close extended', short: 'Soft close' };

/** Live auctions platform-wide, now and yesterday at the same minute. */
export const liveAuctions = { now: 128, yesterday: 118 };
/** Hammer volume since midnight, now and yesterday at the same minute. */
export const hammerVolume = { now: 1_842_600, yesterday: 1_891_400 };
/** Bidders active in the last 30 minutes. */
export const activeBidders = 6_204;

export const health = [
  { name: 'Auction commands', value: '99.99%', detail: 'p99 84 ms', ok: true },
  { name: 'Realtime fan-out', value: '99.97%', detail: 'p99 128 ms', ok: true },
  { name: 'Settlement worker', value: '12 pollers', detail: '38 ms schedule-to-start', ok: true },
  { name: 'Payment adapter', value: '2.1%', detail: 'recovery challenge rate, usual 0.4%', ok: false },
];

export interface PublicBid {
  id: string;
  alias: string;
  resultingPrice: number;
  occurredAt: string;
  isMine?: boolean;
}

export const speedmasterBids: PublicBid[] = [
  { id: 'b-184', alias: 'Silver Fox', resultingPrice: 42_800, occurredAt: '14:31:52', isMine: true },
  { id: 'b-183', alias: 'Atlas 17', resultingPrice: 42_600, occurredAt: '14:31:49' },
  { id: 'b-182', alias: 'North Star', resultingPrice: 42_100, occurredAt: '14:30:08' },
  { id: 'b-181', alias: 'Silver Fox', resultingPrice: 41_800, occurredAt: '14:29:44', isMine: true },
  { id: 'b-180', alias: 'Mistral 4', resultingPrice: 41_500, occurredAt: '14:28:16' },
  { id: 'b-179', alias: 'Atlas 17', resultingPrice: 41_200, occurredAt: '14:27:03' },
];

export const operator = { name: 'Elena Volkova', role: 'Senior operator', initials: 'EV' };

// The figures that must agree, checked once at load.
console.assert(hotLots[0].id === 'GV-2841', 'the hottest lot is the Speedmaster');
console.assert(queue.filter((item) => item.tone === 'danger').length === 1, 'one time-critical case');
console.assert(bidsPerMinute.length === 61, '61 minutes of bids');
console.assert(bidsPerMinute[bidsSpike.at] === 3_112, 'the spike sits at 14:26');

/** Live auctions per minute over the same window: a step series that changes when a lot opens or closes. */
export const liveAuctionsSeries = bidsPerMinute.map((_, index) => {
  if (index < 9) return 121;
  if (index < 17) return 123;
  if (index < 26) return 122;
  if (index < 38) return 125;
  if (index < 47) return 126;
  return 128;
});

/** Hammer volume since midnight, running total, one value per minute of the window. */
export const hammerVolumeSeries = (() => {
  const closesAt: Record<number, number> = { 4: 6_400, 11: 14_900, 19: 9_600, 27: 22_300, 33: 7_800, 41: 31_400, 48: 12_100, 55: 18_600, 58: 9_200 };
  let total = hammerVolume.now - Object.values(closesAt).reduce((sum, v) => sum + v, 0);
  return bidsPerMinute.map((_, index) => { total += closesAt[index] ?? 0; return total; });
})();

console.assert(hammerVolumeSeries[hammerVolumeSeries.length - 1] === hammerVolume.now, 'the hammer series ends on today\'s total');
console.assert(liveAuctionsSeries[liveAuctionsSeries.length - 1] === liveAuctions.now, 'the live series ends on the live count');
