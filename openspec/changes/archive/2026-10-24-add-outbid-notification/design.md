# Design

## Context

См. `proposal.md` — Why и `specs/` — нормативное поведение. Auction уже хранит secret maxima и вычисляет лидера в aggregate, а `placeBid` после применения решения возвращает `outbid`. Private Auction events атомарно кодируются во второй, ACL-protected outbox `gavel.auction.processes.v1`; Processes уже читает этот topic, signal-with-start передаёт owner events в Temporal, а Notify предоставляет идемпотентный `SendNotification` и Mailpit adapter.

Публичные `BidRecorded`/`PriceChanged` намеренно не содержат `user_id`, поэтому по ним нельзя надёжно определить адресата. Settlement workflow нельзя использовать для открытых торгов: он моделирует закрытый аукцион, кандидата и payment rounds. Новый путь должен сохранять успешную ставку независимо от delivery outcome.

## Goals / Non-Goals

**Goals:**

- Получать outbid recipient из authoritative Auction transition, а не реконструировать его в consumer.
- Атомарно связать принятую ставку и restricted intent, сохранив at-least-once transport и exactly-once logical notification.
- Использовать существующие ownership boundaries: Auction решает «кто перебит», Processes — «когда повторить intent», Notify — «как доставить».
- Оставить public GraphQL/events и bidding latency независимыми от Notify.

**Non-Goals:**

- Объединять частые уведомления в digest или хранить пользовательские preferences.
- Добавлять новый публичный статус, Web inbox либо channel кроме существующего email adapter.
- Давать Processes доступ к secret maxima либо вычислять leadership из публичной истории.

## Decisions

### 1. Auction создаёт отдельный private `BidderOutbid` domain fact

В `contexts/auction/src/domain/auction.rs` `place_bid` вычисляет `previous_leader` до изменения и `next_leader` на prospective bids. После `MaximumBidPlaced` добавляется ровно один `BidderOutbid { bidder_id, bid_id }`, когда:

- `next_leader` отличается от существующего `previous_leader` — recipient равен прежнему лидеру;
- лидер не изменился и новый bidder не является `next_leader` — recipient равен новому bidder;
- bidder уже был лидером и остаётся им — событие отсутствует.

Fact является частью event-sourced stream и имеет собственную aggregate version; его `apply` не меняет pricing state. Поэтому retry/idempotency существующего command executor атомарно воспроизводит прежний result и не создаёт второй факт. `accepted_version`, projection и public event versions продолжают ссылаться на итоговую версию всего решения.

Альтернатива — сравнивать aliases по public `BidRecorded` в Processes — отклонена: alias не является private recipient identity, а proxy maximum и предыдущий лидер публично недоступны. Синхронный Auction → Notify RPC отклонён, потому что связывает commit/latency ставки с внешней доставкой и оставляет dual-write gap.

### 2. Existing restricted Auction topic получает additive payload

В `contexts/auction/proto/gavel/auction/v1/auction_process_events.proto` добавляется новый `oneof` payload `ProcessBidderOutbid` с `recipient_id` и `bid_id`; auction id и version остаются envelope metadata. `contexts/auction/src/adapters/outbound/process_events.rs` кодирует его в той же transaction с private event, а `contracts/event-topics.json` добавляет `BidderOutbid` в allowlist `auction-restricted`.

Maximum, current leader identity и contact/provider data в payload отсутствуют. Topic ACL уже разрешает чтение только `service_account:processes`, поэтому отдельный topic и новая credential не нужны. Public proto, GraphQL SDL и Realtime subscriptions не меняются.

Альтернатива — новый outbid topic — отклонена: visibility, owner, producer и единственный consumer совпадают с существующим restricted process contract; отдельный topic увеличит connector/ACL operations без новой security boundary.

### 3. Processes использует отдельный короткий `NotifyOutbid` workflow

`contexts/processes/src/ingress/events.rs` декодирует restricted payload в отдельный `OwnerEvent::BidderOutbid`. Gateway маршрутизирует его в новый workflow type и task queue registration, не сигнализируя `AuctionLifecycle` или `SettleAuction`. Workflow id строится как `notify-outbid/{auction_id}/{aggregate_version}`, request id — из owner event id; workflow выполняет одну Notify activity и сохраняет только safe acknowledgement.

Notification id строится как `outbid/{auction_id}:{aggregate_version}`. Даже если redelivery приходит после завершения workflow и Temporal начинает новый run, Notify canonical fingerprint возвращает прежний terminal result. Activity retry использует существующие bounded timeout/backoff; permanent Notify rejection завершает workflow наблюдаемым failure и не вызывает owner-команды.

Альтернатива — добавить signal в `SettleAuction` — отклонена: она преждевременно создаёт settlement workflow во время открытых торгов и смешивает независимые lifecycle состояния. Прямой вызов Notify из Redpanda consumer отклонён: broker offset нельзя подтверждать до durable intent, а hand-written retry дублирует Temporal.

### 4. Notify расширяет существующий command новым строгим template

В `contexts/notify/proto/gavel/notify/v1/notify_service.proto` новый enum value отображается на domain `Template::BidderOutbidV1`; `contexts/notify/src/application/templates.rs` принимает только обязательный `auction_id` и рендерит `bidder_outbid.v1`. Domain validation вводит allowlist keys для каждого template, поэтому outbid-команда с `maximum`, `leader_id` или любым лишним field отклоняется до repository/channel adapter.

Существующие PostgreSQL aggregate, delivery attempts, terminal outbox, operator read и `platform#send_notifications` используются без миграции. Только Processes workload может вызвать template; browser/BFF/Auction не получают новой permission.

### 5. Consistency, recovery и observability следуют owner boundaries

Commit ставки сохраняет private events, restricted outbox и public outbox в одной PostgreSQL transaction. Debezium/Redpanda и Processes остаются at-least-once; Temporal workflow identity и Notify notification id обеспечивают дедупликацию на каждом эффекте. Delivery failure не компенсирует bid и не меняет Auction.

Метрики добавляют только bounded event/workflow/template/outcome labels. Recipient, bid id, auction id, notification id и email body не попадают в metric labels или обычные логи. Existing quarantine path принимает malformed/unsupported restricted events без raw payload. Process readiness продолжает зависеть от broker, Temporal и Notify RPC; delivery `FAILED` не делает Auction unavailable.

Use-case документация синхронизируется в `contexts/auction/use-cases/place-bid/`, новом `contexts/notify/use-cases/outbid-notification/` и соответствующих `flow.c4`; `docs/architecture/model.c4` показывает restricted event → Processes → Notify без изменения public runtime boundaries.

## Risks / Trade-offs

- **[Частые смены лидера создают много email]** → первая версия отправляет один email на каждый authoritative outbid fact; volume и provider limits наблюдаются отдельно, preferences/digest остаются последующим capability.
- **[Новый no-op private event увеличивает stream version]** → event является business fact с durable delivery requirement; все результаты и public envelopes используют итоговую aggregate version, а tests фиксируют порядок и replay.
- **[Старый Processes не понимает новый payload]** → rollout consumer/Notify выполняется до включения нового Auction producer; unknown payload fail closed/quarantine, а rollback producer прекращает новые события без изменения уже принятых bids.
- **[Recipient id фактически должен быть доставляемым адресом]** → сохраняется текущий local-MVP контракт Notify, где verified user principal является recipient; отдельный contact-resolution capability не вводится этой работой.
- **[Temporal redelivery после завершения workflow]** → Notify id/fingerprint является последним durable idempotency barrier, поэтому повторный run не создаёт второй email.

## Migration Plan

1. Добавить и проверить additive protobuf/registry contract, Notify template и Processes decoder/workflow при ещё не публикующем `BidderOutbid` Auction.
2. Развернуть Notify и Processes, затем Auction producer; schema version и существующие topics сохраняются.
3. Прогнать local MVP сценарии смены лидера, immediate outbid, duplicate delivery и no-notification для повышения собственного maximum.
4. При rollback сначала вернуть Auction producer, затем Processes/Notify. Уже сохранённые notifications остаются audit history; ставки и public history не откатываются.
