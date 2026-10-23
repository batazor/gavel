export type CustomerState = 'Active' | 'Review' | 'Restricted';
export type PaymentState = 'Ready' | 'Missing' | 'Revoked';

export interface Customer {
  id: string;
  name: string;
  email: string;
  since: string;
  region: string;
  state: CustomerState;
  payment: PaymentState;
  bids: number;
  won: number;
  volume: number;
  lastSeen: string;
  /** A stable hue for the person's initial, 0–360, people listed together at least 60° apart. */
  hue: number;
}

/* Sample members on this desk. Replace with the Identity and Payments projections before launch. */
export const customers: Customer[] = [
  { id: 'USR-0834', name: 'Marta Weiss', email: 'marta.weiss@posteo.de', since: 'Mar 2025', region: 'Berlin, DE', state: 'Review', payment: 'Ready', bids: 128, won: 7, volume: 84_200, lastSeen: '2 min ago', hue: 20 },
  { id: 'USR-0811', name: 'Jon Bell', email: 'jon.bell@proton.me', since: 'Feb 2025', region: 'Leeds, UK', state: 'Active', payment: 'Ready', bids: 64, won: 3, volume: 31_800, lastSeen: '18 min ago', hue: 200 },
  { id: 'USR-0772', name: 'Aya Morita', email: 'aya.morita@icloud.com', since: 'Dec 2024', region: 'Osaka, JP', state: 'Active', payment: 'Ready', bids: 203, won: 11, volume: 126_400, lastSeen: '1 h ago', hue: 140 },
  { id: 'USR-0691', name: 'Liam Costa', email: 'liam.costa@gmx.de', since: 'Oct 2024', region: 'Porto, PT', state: 'Restricted', payment: 'Revoked', bids: 18, won: 1, volume: 4_800, lastSeen: '3 days ago', hue: 260 },
  { id: 'USR-0528', name: 'Nora Lind', email: 'nora.lind@outlook.com', since: 'Jun 2024', region: 'Malmö, SE', state: 'Active', payment: 'Missing', bids: 0, won: 0, volume: 0, lastSeen: '5 days ago', hue: 80 },
];

/** Platform-wide member figures, as the Identity projection reports them. */
export const members = { total: 24_812, readyToBid: 18_405, needReview: 38, needReviewHighPriority: 12 };

export const initials = (name: string) => name.split(' ').map((part) => part[0]).join('').slice(0, 2);

/** Why members platform-wide cannot bid right now; the parts sum to total − readyToBid. */
export const blockedReasons = [
  { reason: 'No payment instrument', count: 5_102 },
  { reason: 'Account restricted', count: 1_267 },
  { reason: 'Waiting for review', count: 38 },
];

/** Review decisions recorded on this desk today. */
export const decisionsToday = [
  { time: '13:48', member: 'Tomas Ekberg', id: 'USR-0802', decision: 'Approved', by: 'Elena Volkova' },
  { time: '12:15', member: 'Priya Natarajan', id: 'USR-0795', decision: 'Approved', by: 'Elena Volkova' },
  { time: '10:52', member: 'Felix Brandt', id: 'USR-0788', decision: 'Restricted', by: 'Oskar Lind' },
  { time: '09:30', member: 'Chiara Rossi', id: 'USR-0761', decision: 'Approved', by: 'Oskar Lind' },
];

console.assert(blockedReasons.reduce((sum, r) => sum + r.count, 0) === members.total - members.readyToBid, 'blocked members add up');
