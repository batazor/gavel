# Design

## Context

См. [proposal.md](proposal.md) — Why. Auction, Payments и Notify уже имеют application boundaries и owner gRPC contracts, но Processes содержит только client re-exports, retry classification и синхронный hello-world HTTP server. Текущие Auction public events намеренно privacy-safe и не представляют `SettlementCandidateSelected`; Payments event после callback не содержит recovery URL, поэтому одного существующего публичного event stream недостаточно для settlement orchestration.

`temporalio-sdk` 1.1.0 — официальный Rust SDK с workflow/activity API, deterministic timers/select, patching и local test environment. Он требует Rust 1.92 для SDK crate; Processes Docker builder уже использует Rust 1.98, но crate ошибочно декларирует MSRV 1.85. Workflow API включается feature `experimental`, поэтому dependency должна быть exact-pinned, а upgrade — отдельным проверяемым действием.

## Goals / Non-Goals

**Goals:**

- получить replay-safe `AuctionLifecycle` и `SettleAuction` с явными state machines;
- сохранить Auction, Payments и Notify единственными владельцами доменных решений и данных;
- сделать owner effects идемпотентными при activity retry, workflow replay и worker restart;
- подготовить broker-agnostic signal ingress и owner event contracts для следующего Redpanda/Debezium change;
- проверять workflow history и гонки на реальном local Temporal test server.

**Non-Goals:**

- устанавливать production Temporal cluster или его PostgreSQL внутри service-owned chart;
- реализовывать Redpanda consumer/offset management и Debezium connectors;
- реализовывать `WithdrawLot`, публичный realtime или операторское управление workflow;
- хранить копию Auction aggregate либо самостоятельно определять candidate, price или payment outcome.

## Decisions

### 1. Использовать официальный Rust SDK 1.1.0 и поднять MSRV только Processes

В root workspace dependencies добавляются exact `temporalio-sdk`, `temporalio-client`, `temporalio-common` и `temporalio-macros` 1.1.0; `contexts/processes/Cargo.toml` получает Rust 1.92 и production features `experimental`, `envconfig`, `prometheus`/OTel, а testing feature используется в test target. Другие crates сохраняют текущий MSRV.

Workflow code использует только SDK primitives: workflow time, timers, deterministic `select!`, signals, activities и patch markers. Tokio I/O, system clock, random, network clients и mutable globals разрешены только adapters/activities. Runtime nondeterminism detector остаётся включённым.

Альтернатива — старый pre-1.0 SDK ради Rust 1.85 — отвергнута: она фиксирует Processes на нестабильном API и усложняет replay-compatible upgrades. TypeScript/Go worker отвергнут как второй backend runtime и нарушение принятого Rust-решения.

### 2. Разделить pure workflow state, ingress и side-effect activities

В `contexts/processes/src/` вводятся границы:

- `workflows/auction_lifecycle.rs` и `workflows/settle_auction.rs` — сериализуемое state и deterministic transitions;
- `activities/` — Auction, Payments и Notify ports и Temporal activity implementations;
- `ingress/` — typed owner events, schema/version validation, monotonic routing и `signal-with-start` gateway;
- `adapters/grpc/` — owner-generated tonic clients и trusted workload metadata;
- `bootstrap.rs`, `telemetry.rs` — config, worker registration, health/readiness, metrics и shutdown.

Workflow modules не импортируют tonic, SQLx, HTTP или provider SDK. Architecture boundary tests закрепляют это правило.

Альтернатива — вызывать gRPC прямо из workflow — отвергнута: I/O ломает replay determinism и не даёт Temporal управлять activity timeout/retry.

### 3. Один workflow ID на auction и version-aware signals

IDs фиксированы: `auction-lifecycle/{auction_id}` и `settle-auction/{auction_id}`; Temporal run ID не входит ни в workflow ID, ни в idempotency keys. Ingress использует signal-with-start с conflict policy `USE_EXISTING` и request id, выведенным из owner `event_id`.

Workflow state хранит только минимальную process-копию: последний принятый owner aggregate version/event id, актуальный deadline, текущие round/candidate/amount/attempt id и terminal marker. Candidate и recovery URL не публикуются query handler; workflow query возвращает только safe status/round/deadlines для диагностики.

Duplicate event с тем же fingerprint игнорируется, тот же event id с другим payload переводится в quarantine/error path, меньшая aggregate/attempt version считается stale. Gap в version не блокирует workflow навсегда: ingress отмечает gap metric и выполняет owner reconciliation read, а не угадывает пропущенное состояние.

### 4. AuctionLifecycle — timer loop, а не timestamp sleep вне Temporal

`AuctionOpened`, `DeadlineExtended` и `AuctionRescheduled` изменяют state только при большей Auction version. Workflow создаёт SDK durable timer до `ends_at`; новый deadline логически отменяет старое ожидание через deterministic select и начинает новое.

После timer вызывается `CloseAuction` activity с workflow time и ключом `auction-lifecycle/{id}:close`. Typed `too_early` должен вернуть authoritative deadline; workflow переставляет timer. `SETTLING` завершает lifecycle ожидание и оставляет settlement отдельному workflow, `UNSOLD`/`CANCELLED`/`SOLD` завершают execution.

### 5. Restricted Auction process outbox отделён от public outbox

Auction получает owner contract `auction_process_events.proto` с envelope, event/schema/aggregate versions, correlation/causation и payload `SettlementCandidateSelected`/`AuctionUnsold`/`WinnerConfirmed`/`AuctionCancelled`. Candidate id и settlement amount допустимы только в restricted contract.

Новая additive migration создаёт `auction_process_outbox`; запись private stream, public projection, public outbox и process outbox остаётся одной SQL transaction. Public `auction_outbox` и `AuctionPublicEvent` не расширяются приватными полями. Будущий Debezium connector будет иметь отдельный topic и ACL только для Processes; текущий change проверяет codec, atomic persistence и ingress consumption, но не устанавливает connector.

Альтернатива — добавить candidate в `AuctionPublicEvent` — отвергнута из-за privacy boundary. Чтение private event store Processes также отвергнуто: оно связывает контекст с чужой БД и wire codec.

### 6. Payments предоставляет process-only authoritative attempt read

Payments contract additive получает `GetSettlementAttempt`, принимающий attempt id плюс ожидаемые auction id/round и correlation/consistency context. Response содержит provider-neutral status, version, candidate binding, payment id при успехе и recovery URL только для `RECOVERY_REQUIRED`.

RPC доступен только trusted Processes workload с существующей process permission; application use case читает Payments-owned repository и fail-closed проверяет binding. URL никогда не попадает в payment event, workflow query, metrics или обычные structured logs. Activity немедленно передаёт recovery delivery data в Notify и возвращает workflow только redacted acknowledgement; для повторного уведомления activity повторно читает owner state.

Альтернатива — положить recovery URL в Redpanda event/Temporal state — отвергнута из-за retention, visibility и history exposure.

### 7. SettleAuction обрабатывает round как конечный автомат

Состояния round: `CandidateSelected → Attempting → Processing | Recovering → Confirming | Expiring → Forfeiting → Terminal/NextRound`. Новый round принимается только из restricted Auction event с большей Auction version; workflow не вычисляет amount.

`AttemptPayment` может сразу вернуть `SUCCEEDED`, `RECOVERY_REQUIRED`, `PROCESSING` или ранее сохранённый terminal result. Для `PROCESSING` workflow ждёт versioned Payments signal и после каждого сигнала делает authoritative `GetSettlementAttempt`. Для `RECOVERY_REQUIRED` запускается ровно двухчасовой SDK timer от зафиксированного owner transition time; notification failure timer не сдвигает.

При гонке timer/signal используется deterministic SDK select. Timer сначала вызывает `ExpirePayment`; только authoritative `EXPIRED` разрешает `ForfeitSettlementCandidate`. `ALREADY_SUCCEEDED` или read `SUCCEEDED` ведёт к `ConfirmWinner`. `ForfeitSettlementCandidate` не возвращает candidate details: следующий round приходит отдельным restricted Auction event. Non-terminal `PROCESSING` никогда не превращается в forfeiture по локальному предположению.

### 8. Activities используют один policy и стабильные intent keys

Каждый owner RPC имеет start-to-close timeout 3 секунды и максимум 4 Temporal attempts с exponential backoff и jitter. `UNAVAILABLE`, `RESOURCE_EXHAUSTED`, `DEADLINE_EXCEEDED` retryable; domain rejection и остальные коды маппятся в typed activity outcome/failure согласно `rpc_policy.rs`.

Ключи:

- `auction-lifecycle/{auction}:close`;
- `settle-auction/{auction}:attempt-payment:{round}`;
- `settle-auction/{auction}:expire-payment:{round}`;
- `settle-auction/{auction}:forfeit:{round}`;
- `settle-auction/{auction}:confirm:{round}`;
- `settle-auction/{auction}:notify:{template}:{round}`.

Payload fingerprint у owner защищает повторное использование ключа с изменёнными данными. Correlation/causation переносятся из входного owner event через activities; секреты и entity ids запрещены в metric labels.

### 9. Notify — независимый delivery effect

Settlement, recovery и winner notification вызываются отдельными идемпотентными activities. Временная недоступность повторяется по policy; non-retryable rejection записывается как bounded process outcome, но не меняет Auction/Payments state и не продлевает recovery window. Recovery activity получает URL из Payments внутри activity execution и передаёт его Notify без возврата в workflow state.

### 10. Runtime и readiness отражают способность выполнять workflow

`main.rs` становится Tokio bootstrap. Readiness `200` требует валидной конфигурации, соединения с Temporal namespace и запущенных workflow/activity pollers; liveness отражает только живой process. Потеря Temporal снимает readiness, но не завершает/очищает histories. Graceful shutdown прекращает polling, ждёт bounded drain и закрывает HTTP exporter.

Helm добавляет Temporal address/namespace/task queues из non-secret config, mTLS/API credential только через `secretKeyRef`, topology/PDB и минимум две worker replicas для production values. Service chart не устанавливает Temporal server. `Makefile` получает local Temporal CLI/dev-server lifecycle и `integration-check`; production cluster остаётся обязанностью platform deployment change.

### 11. Наблюдаемость и replay compatibility являются release gates

Используется уже принятый contract из `contexts/processes/observability/`: SDK metrics плюс bounded workflow/activity outcome metrics, overdue watchdog вне workflow и safe logs/traces. Workflow history не содержит recovery URL, provider refs или bid maxima.

Каждое изменение control flow после релиза использует SDK patch marker. Tests сохраняют representative history fixtures для lifecycle reschedule, recovery success, expiry/forfeiture и worker restart; новый worker обязан replay их без nondeterminism до rollout.

## Risks / Trade-offs

- **[Rust workflow API всё ещё feature-gated как experimental]** → exact pin 1.1.0, history replay suite, isolated Processes MSRV и отдельный dependency-upgrade change.
- **[Processes MSRV выше остальных crates]** → изменить только `contexts/processes`, использовать уже существующий Rust 1.98 builder и явно проверить clean image build.
- **[Restricted process outbox содержит user identity и amount]** → отдельная таблица/contract/topic ACL, encryption in transit/at rest, короткий retention и запрет public/realtime consumers.
- **[Broker adapter ещё не реализован]** → ingress/codec/Temporal gateway проверяются contract tests; production включение workflows блокируется до Redpanda/Debezium change, ручной ingress отсутствует.
- **[Recovery URL может попасть в Temporal history через activity result]** → activity возвращает только redacted acknowledgement; URL живёт лишь внутри activity memory и Notify request.
- **[Signal gap или out-of-order delivery]** → monotonic versions, gap metric и authoritative reconciliation read; workflow не синтезирует пропущенный outcome.
- **[Long-running histories растут]** → lifecycle и settlement разделены; завершать каждый auction execution, а continue-as-new рассматривать только если load test покажет чрезмерную history.
- **[Notification и domain progression расходятся]** → deterministic notification ids и независимые retries; delivery не становится условием domain correctness.

## Migration Plan

1. Additive выпустить Auction restricted event proto/table/codec и Payments process-read RPC; прогнать Buf breaking, owner contract/privacy и persistence tests.
2. Поднять Processes MSRV/dependencies, реализовать workflow/activity/ingress boundaries и local Temporal integration suite.
3. Обновить Processes image/Helm/observability и развернуть worker с новым task queue без production event ingress; выполнить synthetic workflow и history replay.
4. Canary worker подключить к staging Temporal namespace, проверить timer/restart/recovery races и dashboards.
5. В следующем Redpanda/Debezium change подключить restricted topics к ingress, начать с shadow/reconciliation режима и только затем включить production signal-with-start.

Rollback: остановить новые worker replicas и вернуть предыдущую совместимую build; workflow histories и owner outboxes не удалять. Если history уже содержит новую patch branch, сохранить как минимум одну совместимую worker deployment до завершения executions. Additive schema/proto поля и таблицу не откатывать деструктивно.

## Open Questions

- Конкретный production Temporal Server chart/version и Visibility backend выбираются platform deployment change; этот design требует лишь совместимый endpoint и namespace.
- Retention restricted Auction process topic определяется вместе с Redpanda capacity/ACL design, не меняя workflow contract.
