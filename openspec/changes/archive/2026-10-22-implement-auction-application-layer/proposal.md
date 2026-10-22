# Proposal

## Why

Чистый агрегат Auction уже защищает правила торгов, но сервис пока остаётся bootstrap-заглушкой: команды, event store, outbox, идемпотентность, read model и публичные adapters не соединены в исполняемый вертикальный срез. Без application layer невозможно безопасно принять конкурентную ставку, обслужить lifecycle-команды Processes или доказать, что секретные максимумы не покидают authoritative хранилище.

## What Changes

- Добавить application use cases Auction для открытия торгов из `LotListed`, размещения ставки, переноса дедлайна, закрытия, отмены, forfeiture settlement candidate и подтверждения победителя.
- Ввести явные ports для event store, transactional outbox, idempotency, authorization, payment-readiness projection, public projections, clock/transaction boundary и cache; доменный модуль не получает инфраструктурных зависимостей.
- Реализовать service-owned PostgreSQL schema и adapters для приватного event stream, optimistic append, snapshots, idempotent command results, outbox и authoritative current-price projection.
- Реализовать входные GraphQL и gRPC adapters, маппинг стабильных typed outcomes и проверку доверенной identity/permissions до доменной команды.
- Реализовать Auction-owned consumers контрактных событий Catalog и Payments, а также version-aware L1/Redis/fallback чтение публичной цены и projector безопасных публичных событий.
- Заменить hello-world runtime на конфигурируемый сервис с HTTP/GraphQL, internal gRPC, readiness и telemetry; добавить domain/application/adapter/contract/integration tests и migrations.
- Зафиксировать публичные event envelopes Auction так, чтобы `maximumAmount`, `user_id`, private leader identity и storage keys никогда не попадали в broker, GraphQL public fields или метрики.

## Capabilities

### New Capabilities

- `auction-bidding`: открытие торгов, авторизованная и идемпотентная постановка proxy bid, перенос дедлайна, конкурентный optimistic retry и безопасные публичные результаты/события.
- `auction-lifecycle-settlement`: внутренние lifecycle-команды закрытия, отмены, forfeiture и подтверждения победителя с authoritative typed outcomes и атомарной публикацией фактов.
- `auction-current-price`: публичное version-aware чтение текущей цены через L1, Redis и authoritative PostgreSQL projection без использования cache в командном пути.

### Modified Capabilities

Нет: существующий `internal-rpc-contracts` уже задаёт обязательные свойства внутренних RPC и реализуется без изменения его требований.

## Impact

- **Primary owner:** bounded context Auction; основные пути — `contexts/auction/src/`, `contexts/auction/migrations/`, `contexts/auction/graphql/`, `contexts/auction/proto/`, `contexts/auction/ops/` и Auction use-case/ADR документация.
- **Impacted contexts:** Catalog остаётся authoritative владельцем `LotListed`; Payments — `PaymentInstrumentReady`/`PaymentInstrumentRevoked` и `PaymentSucceeded`; Authz — permission decisions; Processes — caller существующего Auction gRPC v1; BFF/Cosmo Router — consumer существующего Federation SDL; Realtime и Analytics — consumers безопасных Auction events. Их внутренние модели вне scope.
- **Contracts:** используются существующие GraphQL SDL и Auction gRPC v1; добавляются versioned Auction public event schemas и service-owned database migrations. Несовместимых изменений публичных GraphQL/gRPC контрактов не планируется.
- **Consistency and compatibility:** private events, outbox rows, idempotency result и authoritative projection фиксируются в одной PostgreSQL transaction; optimistic conflict повторяет decision на свежем state; consumers и projectors остаются at-least-once и version/idempotency aware.
- **Out of scope:** реализация Temporal workflows, Payments/Catalog/Authz серверов, Realtime/Analytics consumers, production credentials и deployment внешней инфраструктуры; этот change предоставляет и проверяет их контрактные точки интеграции.
