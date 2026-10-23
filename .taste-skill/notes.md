# Gavel UI — taste notes (app screens run)

Style dealt: **achromatic-ledger**, seed 1728568553, kind commerce, light, `--keep palette`.
Why: operators work the back office in long sessions and most of what they read is money and counts;
the auction house's own artefact is the ledger (the auction book). Colour goes on the data, the chrome
stays ink on paper. The copper brand hue moves off the buttons onto the figures that matter.

## Face (chosen before building)
1. Ground from the product's world: a warm stone paper (#f4f2ee), not the yellow cream (#f5f0e8). Cards near-white.
2. The brand copper (#bd5634 / #984329 for small text) on the main data element only: the live/hammer price,
   today's bids-per-minute line, the recovery deadline. Under 1% of pixels.
3. Geist for text, Geist Mono for IDs (GV-2841, STL-4912) only. Tabular figures everywhere.

## Signature element
**The closing board**: lots closing in the next hour on one 60-minute strip — scheduled end as an ink dot,
the soft-close extension as a lighter segment after it, "now" as a 1px copper line. On the overview's first screen.

## Inventory (data and jobs)
- Objects: lots (id GV-xxxx, title, seller, category, current price €, bids, watching, ends, state: live/review/draft/closed),
  customers (USR-xxxx, name, email, state, payment readiness, bids, won, volume), settlement cases (STL-xxxx, amount,
  candidate alias, round, deadline), queue items (kind, object, due).
- Magnitudes: 128 live auctions platform-wide; ~2,000–2,500 accepted bids/min at peak; hammer prices €5k–€45k; a desk watches 6 lots.
- People: operators (Elena Volkova, senior operator) in 8-hour sessions at 1440px; bidders on phones for the bid panel.
- Jobs: review a lot, approve/restrict a customer, open a payment recovery, place a bid (public panel), publish a lot.
- Product's own things: the gavel mark, the desk scope (Operations · Berlin desk), bidder aliases (Silver Fox, Atlas 17).

## Screen questions
- Overview — "Is anything waiting on an operator right now, and are the live auctions healthy?"
  Finding: payment recovery STL-4912 expires at 16:27, 1 of 7 open cases is time-critical. Primary: Open inbox.
- Lots — "Which lots are live and which must be reviewed before they can go live?" Primary: Create lot.
- Customers — "Who is blocked from bidding (review, payment missing)?" Primary: none (row actions).
- Customer profile — "Should this customer keep bidding?" Primary: Approve.
- Create lot — "Is this lot ready to publish?" Primary: Publish lot.
- Live bid panel — "Am I leading, and what does the next bid cost?" Primary: Place bid.
- Settlement recovery — "Will this candidate pay before the window closes?" Primary: Open recovery.

## Before (inspect --app, overview): 5 FAIL / 4 WARN
micro-label ×12, font-not-dealt, heavy-shadow ×8, tiny-text ×78, no-title; cream palette, no tabular figures, icon tiles.

## After (inspect --app, final round)
Overview, Lots, Customers, Customer profile, Create lot: **0 FAIL / 0 WARN** at 1440, 768 and 390
(Create lot: one `ragged-row` WARN defended — the preview is a sticky right rail (§3.4), not a row-mate).
Live bid panel, Settlement recovery: 0 FAIL with `--allow no-title` (component stories carry an h2; the h1 belongs to the page).

Fixes found only by looking (round 1 → 3): unlayered `button { color: inherit }` was beating Tailwind's utilities (primary
buttons rendered ink on black); Storybook's own placeholder h1 was read as the title (story root now first in the DOM);
identity columns truncated at 1440 (fixed width share, state pill folded into the phone subline); closing-board axis
crowded in a half column (three labels); cream check (ground moved to #f4f3f1, surface to white); copper deepened to
#9f4428 so figures hold 6:1 and leave the rust band; Customers and Create lot first screens were a third empty
(blocked-members and decisions panels; the photograph guide).

## Critique (references/app-screens.md §2.7), last round
1. Question answered by the biggest block: yes on every screen (bids chart with KPI tabs; the lots table; the member table;
   the object header + evidence; the form sheet). Largest figure = the title row's sentence (2,418 bids/min; €42,800).
2. Edges: one left edge from h1 to the last row; rows share bottom edges (overview pairs within 5px); no dead strips.
3. Colour: copper only on the live price, today's series, the recovery countdown; status pills pale; one near-black primary.
4. Numbers: totals from rows (queue 7 = 4 shown + 3; blocked 6,407 = 5,102 + 1,267 + 38; hammer series ends on today's total,
   asserted at load); every change names its baseline ("vs yesterday 14:32").
5. Zeros print as grey 0 / €0; missing as "—"; the count on the nav is only on Settlements (2 cases).
6. Legend swatches are the marks (closing board: dot, copper segment, now line). No caption explains bars.
7. Phone: sidebar folds to a 48px bar + sheet; KPI tabs become a 2-up grid with the odd cell spanning; tables fold state and
   seller into the subline; the bid panel stacks form over history.
Still short: lot photographs are flat slots (no honest picture exists); the Prototype labs keep the old composition.

## Brand chrome restored (2026-10-23, after review)
The owner asked for the old character back. Kept from the run: the composition of every screen, the KPI strip, the tables,
the data rules, the states, the closing board, document scroll. Restored as the brand's own devices, allowed on purpose:
parchment ground #f5f0e8 and paper cards (`palette-cream`), Avenir Next / Figtree instead of the dealt Geist (`font-not-dealt`),
the tracked GAVEL wordmark (`micro-label`), a dark ink sidebar with the copper mark, the copper primary button,
12px panels with a faint warm shadow (0 4px 14px at 6%), a copper meta line over panel titles.
Inspect with `--allow font-not-dealt,micro-label,palette-cream,palette-claude`.
