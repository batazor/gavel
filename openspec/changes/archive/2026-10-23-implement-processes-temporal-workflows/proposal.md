# Proposal

## Why

Auction, Payments и их owner-контракты уже реализуют authoritative доменные переходы, но `contexts/processes` пока остаётся hello-world scaffold и не выполняет обещанную durable-оркестрацию. Нужен Temporal worker, который переживает рестарты, корректно управляет дедлайнами и recovery window и доводит закрытый аукцион до `SOLD` либо `UNSOLD`, не перенося доменные решения из сервисов-владельцев.

## What Changes

- Реализовать в Processes детерминированный `AuctionLifecycle` workflow: старт по `AuctionOpened`, перестановка durable timer по versioned deadline events и идемпотентный вызов `CloseAuction`.
- Реализовать `SettleAuction` workflow: запуск по `SettlementCandidateSelected`, `AttemptPayment`, обработка `PROCESSING`/`RECOVERY_REQUIRED`/`SUCCEEDED`/`EXPIRED`, двухчасовой recovery timer, `ConfirmWinner` либо `ForfeitSettlementCandidate` и переход к следующему round.
- Реализовать Temporal activities поверх owner-generated Auction, Payments и Notify gRPC clients с едиными deadline, retry classification, стабильными idempotency keys и correlation context.
- Добавить принадлежащий Auction restricted process-event contract для `SettlementCandidateSelected`/terminal settlement facts, потому что публичный `AuctionPublicEvent` намеренно не может содержать candidate identity или settlement amount.
- Additive расширить Payments owner RPC безопасным чтением authoritative attempt outcome и recovery delivery data для Processes после асинхронного callback; recovery URL не публиковать в брокерные события.
- Добавить version-aware signal ingress, который дедуплицирует и упорядочивает owner events перед передачей workflow; адаптер Redpanda/Debezium остаётся отдельным изменением и подключится к этому ingress port позже.
- Заменить Processes hello-world на production-shaped Tokio/Temporal worker runtime с health/readiness, graceful shutdown, безопасной конфигурацией и bounded telemetry.
- Обновить Processes Helm/observability artifacts, use-case Mermaid и LikeC4 flow в соответствии с реализованным поведением.
- Добавить deterministic workflow/activity tests и сквозные contract tests с существующими Auction, Payments и Notify interfaces.

## Capabilities

### New Capabilities

Нет.

### Modified Capabilities

- `auction-lifecycle-settlement`: добавить обязательное поведение durable lifecycle/settlement workflows, обработку versioned сигналов, recovery timer, повторов, рестартов и terminal convergence.

## Impact

- **Primary owner:** Processes (`contexts/processes/`) владеет workflow definitions, timers, activity policy, runtime и process telemetry.
- **Impacted bounded contexts:** Auction остаётся владельцем дедлайна, кандидата, settlement amount и terminal статуса; Payments — владельцем платёжной попытки и её terminal outcome; Notify — владельцем доставки settlement/recovery уведомлений. Их доменные правила и хранилища не переносятся в Processes.
- **Cross-context contracts:** добавляется отдельный restricted Auction process-event contract и additive Payments process-read RPC; существующие публичные Auction events, Payments events и Notify RPC остаются совместимыми. Изменения сначала вносятся у владельца и проверяются Buf/Federation compatibility.
- **Infrastructure:** добавляется Temporal client/worker dependency, namespace/task queues, Helm configuration, readiness и workflow observability. Temporal history не становится вторым event store Auction.
- **Compatibility and rollout:** workflow IDs и idempotency keys стабильны между retries/restarts; rollout должен поддерживать безопасный worker versioning и воспроизведение уже начатой history. Breaking API или event migration не планируется.
- **Out of scope:** Redpanda/Debezium deployment и реальный broker consumer, Realtime/BFF subscriptions, WithdrawLot saga, refunds/payouts, выбор следующего кандидата или расчёт суммы вне Auction, provider-specific логика вне Payments.
