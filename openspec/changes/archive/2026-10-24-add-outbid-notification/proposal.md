# Proposal

## Why

Участник видит потерю лидерства только при повторном открытии рабочего пространства или через live price updates, но не получает отдельного уведомления. Для аукциона с proxy bidding нужен надёжный сигнал «вас перебили», который отправляется ровно тому участнику, чья принятая ставка после пересчёта не лидирует, без раскрытия чужого maximum или identity.

## What Changes

- Auction определяет outbid как результат каждого успешно зафиксированного `placeBid`: либо новый участник немедленно остаётся не-лидером из-за существующего proxy maximum, либо прежний лидер теряет лидерство.
- Auction атомарно с private stream публикует restricted `BidderOutbid` для Processes; событие содержит только auction id, получателя, причинивший переход bid id и authoritative aggregate version и не попадает в public topic.
- Processes запускает отдельный короткий durable workflow уведомления, дедуплицирует повторную доставку owner event и вызывает Notify с детерминированным notification id.
- Notify принимает новый versioned template `bidder_outbid.v1`, рендерит email только с безопасными данными аукциона и сохраняет существующие идемпотентность, retry и operator-read гарантии.
- Local MVP и contract/e2e проверки подтверждают оба варианта: потерю лидерства прежним лидером и немедленный outbid нового участника; повышение maximum текущим лидером не создаёт уведомление.
- Обновляются use-case Mermaid/LikeC4, registry restricted events, operational telemetry и статус юзкейса в `contexts/README.md`.

## Capabilities

### New Capabilities

- `outbid-notification-delivery`: определяет restricted outbid intent, durable orchestration и идемпотентную provider-neutral доставку уведомления перебитому участнику.

### Modified Capabilities

- `auction-bidding`: успешная ставка теперь создаёт restricted факт `BidderOutbid` ровно при определённой Auction потере лидерства, сохраняя secret maxima и private identities вне public contracts.

## Impact

- **Primary owner:** Auction для решения, кто действительно перебит; основные пути — `contexts/auction/src/domain`, restricted process proto/outbox и `contexts/auction/use-cases/place-bid/`.
- **Impacted contexts:** Processes владеет durable orchestration/retry нового уведомления; Notify владеет шаблоном, каналом, дедупликацией и delivery history. Web/BFF не меняют бизнес-правила и публичный GraphQL contract.
- **Cross-context contracts:** additive `BidderOutbid` в `gavel.auction.processes.v1`; новый разрешённый Notify enum/template `bidder_outbid.v1`; существующий gRPC `SendNotification` остаётся транспортом Processes → Notify.
- **Compatibility/migration:** protobuf additions используют новые field/enum numbers и обратно совместимы; schema version и topic сохраняются. Новая private event variant должна читаться после rollout Auction, а Processes/Notify разворачиваются до producer enablement. Миграция БД не требуется.
- **Out of scope:** push/SMS/in-app inbox, пользовательские notification preferences, digest/rate limiting, уведомление наблюдателей, раскрытие текущего лидера или maximum, изменение public GraphQL/event contracts и уведомления о rejected bids.
