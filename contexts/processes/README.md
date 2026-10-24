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

- Worker `AuctionLifecycle` poll-ит task queue `auction-lifecycle-v1`, принимает signal `owner_event`, держит заменяемый durable timer и вызывает `close_auction`.
- Worker `SettleAuction` poll-ит task queue `settle-auction-v1`, принимает signal `owner_event` и выполняет `attempt_payment`, `get_settlement_attempt`, `expire_payment`, `forfeit_candidate`, `confirm_winner`, `notify` и `notify_recovery`.
- Production ingress читает public и restricted owner topics из Redpanda стабильной consumer group, валидирует versioned envelopes и доставляет их в Temporal через `signal-with-start`. Offset фиксируется только после принятого сигнала либо durable quarantine/DLQ publish.
- Activities вызывают только owner-generated [gRPC contracts](../../docs/adr/0018-internal-rpc-with-grpc-and-buf.md) Auction, Payments и Notify.
- Повторы, перестановка событий и потерянный RPC response безопасны: workflow дедуплицирует owner events, отбрасывает stale version, а activities повторяют тот же owner intent с тем же idempotency key.

### Delivery и readiness

- Consumer использует group `gavel-processes-v1`, `read_committed` и cooperative rebalance; auto commit/store отключены.
- `/readyz` успешен только при доступных Temporal, всех четырёх broker topics, активном consumer loop, готовых Auction/Payments/Notify gRPC channels и двух worker pollers.
- Ответ readiness содержит только boolean status зависимостей и число workers. Endpoints, credentials, entity ids и event payload в него не попадают.
- Метрика `gavel_process_ingress_delivery_total` ограничена labels `owner`, `route`, `outcome`; quarantine/replay не экспортирует candidate identity, amount или recovery URL.

### Payments authoritative read

- После versioned Payments signal activity вызывает `GetSettlementAttempt(attempt_id, expected_auction_id, expected_round)`; сигнал сам по себе не является основанием для доменного перехода.
- Payments сверяет immutable auction/round binding и Authz permission для trusted Processes SPIFFE workload. Несовпадение, stale round и недоступный Authz обрабатываются fail-closed.
- Response provider-neutral: candidate binding и version доступны для проверки; recovery URL присутствует только в `RECOVERY_REQUIRED`, payment id — только в `SUCCEEDED`.
- Recovery URL не записывается в workflow state/history, query, search attributes, метрики или логи. Activity передаёт его Notify и возвращает workflow только redacted acknowledgement.

### RPC policy activities

- Каждый вызов имеет deadline 3 секунды и не более 4 attempts с exponential backoff и jitter.
- `UNAVAILABLE`, `RESOURCE_EXHAUSTED` и `DEADLINE_EXCEEDED` повторяются; ambiguous timeout повторяет тот же запрос с тем же ключом.
- `INVALID_ARGUMENT`, `FAILED_PRECONDITION`, `NOT_FOUND`, `PERMISSION_DENIED` и typed domain rejection являются ожидаемыми outcomes workflow и автоматически не повторяются.
- Idempotency key строится из стабильного `workflow_id`, логической операции и settlement round; Temporal run id в ключ не входит.
- Исполняемая классификация и owner client imports находятся в [`src/rpc_policy.rs`](src/rpc_policy.rs) и [`src/lib.rs`](src/lib.rs).

## Инфраструктура
Temporal server self-hosted и использует собственную Postgres. Namespace, endpoint и обе task queue задаются через `TEMPORAL_*`; они не связаны с базами owner-контекстов. Redpanda consumer настраивается через `REDPANDA_*`, а production activities используют `AUCTION_GRPC_ENDPOINT`, `PAYMENTS_GRPC_ENDPOINT` и `NOTIFY_GRPC_ENDPOINT`.

Метрики и SLO процесса описаны в [observability contract](observability/README.md). Processes владеет своими [Helm chart, Prometheus rules и Grafana dashboard](ops/Helm/README.md); общий chart устанавливает только monitoring platform.

## Operations

- Runtime: Redpanda consumer, два worker poller-а, dependency monitors, `/healthz`, `/readyz`, `/metrics`, graceful drain — [`src/main.rs`](src/main.rs).
- Local Helm overlay: [`ops/Helm/values-local.yaml`](ops/Helm/values-local.yaml); broker credentials берутся только из Secret `gavel-redpanda-processes`.
- Локальные проверки: `make -C contexts/processes check`; synthetic Temporal: `make -C contexts/processes integration-check` (Temporal CLI `v1.8.3`, скачивается в локальный cache test runtime).
- Deployment: [Docker image, Helm chart, Prometheus rules и Grafana dashboard](ops/Helm/README.md).
