# Processes — Temporal

Слой процессов над контекстами. Правило: **факты идут в брокер, намерения во времени — в Temporal.**

Workflow состояние торгов не хранит. Он ждёт таймеры, принимает сигналы из брокера и шлёт идемпотентные команды в доменные сервисы. Если агрегат команду отверг, workflow это проживает.

## ADR

- [0001 — Инициализация проекта](adr/0001-init.md)
- [0002 — Наблюдаемость workflow и production paging](adr/0002-workflow-observability.md)

## Interface

- [Federation subgraph SDL](graphql/schema.graphql) — узкий adapter для запуска process commands и чтения `Operation`; Cosmo Router не обращается к Temporal напрямую.
- Owner gRPC contracts: [Auction](../auction/proto/gavel/auction/v1/auction_service.proto), [Catalog](../catalog/proto/gavel/catalog/v1/catalog_service.proto), [Payments](../payments/proto/gavel/payments/v1/payments_service.proto), [Notify](../notify/proto/gavel/notify/v1/notify_service.proto); правила consumption — в [init ADR](adr/0001-init.md).

## Workflow

| Workflow | Старт | Что делает | Документация |
|---|---|---|---|
| `AuctionLifecycle` | `AuctionOpened` | Durable-таймер на `ends_at`, сброс по `DeadlineExtended` и `AuctionRescheduled`, по срабатыванию `CloseAuction(now)` | [close-by-deadline](../auction/use-cases/close-by-deadline/README.md) |
| `WithdrawLot` | Processes subgraph, запрос продавца через Cosmo | `CancelAuction` → `WithdrawLot`, компенсация `ReopenAuction` | [withdraw-lot](../catalog/use-cases/withdraw-lot/README.md) |
| `SettleAuction` | `SettlementCandidateSelected` | Автоматическая попытка через Payments; при `PaymentRecoveryRequired` — 2ч recovery, затем forfeiture и следующий кандидат до `WinnerConfirmed` или `AuctionUnsold` | [settle-auction](use-cases/settle-auction/README.md) |

## Компоненты worker
- **Event subscriber** — читает брокер, стартует workflow и шлёт сигналы.
- **Activities** — вызывают owner-generated [gRPC contracts](../../docs/adr/0018-internal-rpc-with-grpc-and-buf.md) Auction, Catalog, Payments и Notify.

### RPC policy activities

- Каждый вызов имеет deadline 3 секунды и не более 4 attempts с exponential backoff и jitter.
- `UNAVAILABLE`, `RESOURCE_EXHAUSTED` и `DEADLINE_EXCEEDED` повторяются; ambiguous timeout повторяет тот же запрос с тем же ключом.
- `INVALID_ARGUMENT`, `FAILED_PRECONDITION`, `NOT_FOUND`, `PERMISSION_DENIED` и typed domain rejection являются ожидаемыми outcomes workflow и автоматически не повторяются.
- Idempotency key строится из стабильного `workflow_id`, логической операции и settlement round; Temporal run id в ключ не входит.
- Исполняемая классификация и owner client imports находятся в [`src/rpc_policy.rs`](src/rpc_policy.rs) и [`src/lib.rs`](src/lib.rs).

## Инфраструктура
Temporal server self-hosted в namespace `processes` со своей Postgres. Не путать с базами контекстов.

Метрики и SLO процесса описаны в [observability contract](observability/README.md). Processes владеет своими [Helm chart, Prometheus rules и Grafana dashboard](ops/Helm/README.md); общий chart устанавливает только monitoring platform.

## Operations

- [Hello-world build scaffold](src/main.rs)
- [Docker image, Helm chart, Prometheus rules и Grafana dashboard](ops/Helm/README.md)
