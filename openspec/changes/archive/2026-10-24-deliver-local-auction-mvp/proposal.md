# Proposal

## Why

Gavel уже содержит основные доменные правила Auction, Payments и Processes, но их нельзя запустить как один пользовательский продукт: локальный `kind` runtime отсутствует, BFF и Notify остаются bootstrap-сервисами, а production-shaped доставка owner events в Processes не подключена. Нужен минимальный вертикальный срез, который одной командой поднимает систему и позволяет пройти аукцион от опубликованного лота до подтверждённого либо отклонённого settlement.

## What Changes

- Собрать воспроизводимый локальный `kind` runtime для обязательных MVP-компонентов, миграций, seed-данных и readiness-проверок с одной командой запуска и остановки.
- Подключить owner outbox Auction и Payments через Debezium и Redpanda к durable ingress Processes с ограниченными retry, сохранением offsets, quarantine/DLQ и безопасным replay.
- Подключить Catalog `LotListed` и Payments instrument-readiness outbox через отдельные broker routes к реальным Auction consumers, чтобы опубликованный lot открывал аукцион, а допуск к ставке использовал authoritative readiness без прямой записи в Auction storage.
- Заменить BFF bootstrap реальным Cosmo Router execution path и подключить существующие service-owned GraphQL subgraphs к Web для чтения lot workspace, размещения ставки и отображения terminal outcome.
- Реализовать Notify owner gRPC endpoint, durable дедупликацию и email-доставку в локальный Mailpit; сбой уведомления не меняет Auction или Payments outcome.
- Добавить детерминированный sandbox payment adapter и подготовленные локальные identities/relationships, достаточные для двух участников и продавца без live-платежей и внешнего OAuth UI.
- Добавить единый финальный smoke/E2E пакет: happy path, переход к следующему кандидату после неуспешной оплаты и перезапуск Processes без повторных эффектов.
- Использовать polling для обновления workspace в MVP; realtime subscriptions остаются совместимым контрактом, но не входят в критерии готовности этого change.

## Capabilities

### New Capabilities

- `auction-participation-workspace`: публичный GraphQL/Web путь от просмотра опубликованного лота и ставки до наблюдения terminal результата без обхода BFF.
- `process-event-delivery`: надёжная доставка публичных и restricted owner events из transactional outbox в соответствующий Temporal workflow Processes.
- `settlement-notification-delivery`: идемпотентная owner-команда уведомления, история доставки и локальная email-доставка settlement-сообщений.
- `local-development-runtime`: воспроизводимый локальный запуск MVP, seed-сценарий, readiness и единая сквозная проверка.

### Modified Capabilities

Нет. Действующие требования `catalog-lot-discovery`, `auction-bidding`, `auction-current-price`, `auction-lifecycle-settlement`, `payment-attempt-lifecycle`, `authz-permission-decisions` и `internal-rpc-contracts` реализуются сквозным runtime без изменения их нормативного поведения.

## Impact

- **Primary integration owner:** `deploy/` владеет локальным runtime, платформенными manifests, bootstrap и smoke orchestration; доменная семантика и данные остаются у service owners.
- **Catalog:** service-owned GraphQL lot query и seed опубликованного лота; Catalog остаётся authoritative владельцем карточки и lifecycle лота.
- **Auction:** GraphQL mutation/read model, public и restricted outbox contracts; Auction остаётся authoritative владельцем ставок, цены, дедлайна, кандидатов и terminal статуса.
- **Payments:** sandbox provider, readiness seed, attempt events и process RPC; Payments остаётся authoritative владельцем payment attempt.
- **Processes:** Redpanda ingress, routing, offsets/replay и Temporal workflows; Processes оркестрирует, но не принимает доменные решения владельцев.
- **Notify:** gRPC command, собственное хранилище дедупликации/history и Mailpit adapter; Notify владеет delivery outcome.
- **BFF и Web:** Cosmo Router composition, persisted operations и пользовательский workspace; Router не получает бизнес-правил.
- **Authz:** локальные identities и SpiceDB relationships для продавца, двух участников и trusted workloads; permission decisions остаются authoritative в Authz.
- **Cross-context contracts:** существующие Federation SDL, Buf/BSR protobuf contracts, Catalog public lifecycle events, Payments instrument-readiness events, Auction restricted process events, Payments public attempt events и SpiceDB schema. Новые поля добавляются только backward-compatible; секретные максимумы, candidate identity в public events, provider objects и storage keys не публикуются.
- **Migration/compatibility:** production deployments не переключаются этим change; локальные manifests и конфигурация добавляются отдельно, а существующие API contracts сохраняются.
- **Out of scope:** Cloudflare/public edge, production staging и HA, live Stripe, внешний OAuth UX, realtime subscriptions как acceptance gate, Analytics/ClickHouse, refunds/payouts, search, moderation и нагрузочные испытания.
