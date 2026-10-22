# Design

## Context

См. `proposal.md` — Why. `contexts/payments/` содержит SDL, owner Protobuf, public readiness fixtures, README/LikeC4 и hello-world binary, но не содержит домена, application layer, storage или исполняемых adapters. Существующие Authz, Auction и Catalog задают репозиторный шаблон Rust modules, ports/adapters, durable receipts, transactional outbox, trusted workload identity и deployment checks.

Payments находится между двумя независимыми источниками истины: Processes передаёт intent settlement, а внешний provider подтверждает исход операции. Локальный `payments-db` является authoritative источником бизнес-состояния Gavel, но не может атомарно commit вместе с provider API; поэтому каждый внешний effect обязан иметь стабильный provider idempotency key и локальное recoverable состояние.

## Goals / Non-Goals

**Goals:**

- Создать глубокие domain/application modules с малыми interfaces, через которые одинаково работают production adapters и deterministic fakes.
- Защитить инварианты consent, one-attempt-per-round, one-success-per-auction, monotonic transitions и replay safety.
- Сделать provider uncertainty явным retryable состоянием, не выдавая неизвестный результат за failure, expiry или success.
- Сохранить owner boundaries: Payments исполняет payment commands, Processes оркестрирует, Auction считает settlement, Authz принимает permission decisions.
- Поставить production-shaped PostgreSQL, Stripe, GraphQL, gRPC, callback ingress, telemetry и Helm runtime с интеграционными проверками.

**Non-Goals:**

- Реализация Temporal `SettleAuction`, расчёт следующего кандидата или settlement amount.
- Refunds, payouts, penalties, chargeback/dispute lifecycle и второй provider adapter.
- Приём либо хранение карточных реквизитов и синхронная зависимость Auction от Payments на bid hot path.

## Decisions

### 1. Domain и application остаются независимыми от adapters

В `contexts/payments/src/domain/` будут pure типы/агрегаты `PaymentInstrument`, `DebitAuthorization`, `PaymentAttempt`, typed ids, `Money` и domain events. `PaymentInstrument` использует `PENDING → READY → REVOKED`; `PaymentAttempt` — `PENDING → PROCESSING | RECOVERY_REQUIRED | SUCCEEDED | EXPIRED` с явно перечисленными допустимыми переходами.

`contexts/payments/src/application/` предоставляет use cases для регистрации/отзыва, callback application, attempt/continue/expire и query. Узкие ports покрывают unit of work, provider gateway, Authz, clock/id generation и telemetry. GraphQL, tonic, PostgreSQL и Stripe реализуются в `src/adapters/`; `src/bootstrap.rs` только связывает modules.

Альтернатива — поместить lifecycle в resolvers/provider adapter — отклонена: инварианты и replay semantics размножились бы по входам и стали бы зависеть от Stripe vocabulary.

### 2. Два агрегата, одна database-level uniqueness guard

`PaymentInstrument` принадлежит user id и включает authorization snapshot, provider kind, opaque refs и safe display. `PaymentAttempt` принадлежит `(auction_id, settlement_round)`, хранит candidate, immutable money, provider operation ref, lifecycle state и version. Они не объединяются: registration и settlement имеют разные lifecycle/access patterns.

PostgreSQL migration в `contexts/payments/migrations/` создаёт profiles/instruments, attempts, command receipts, provider callback inbox и outbox. Unique constraint гарантирует одну attempt на `(auction_id, settlement_round)`, а partial unique index на `auction_id WHERE status='SUCCEEDED'` — не более одного success. Optimistic version и row locks сериализуют callback/expiry races.

Альтернатива — один агрегат пользователя со всеми attempts — отклонена из-за contention, неограниченного роста и несовпадения ownership ключей.

### 3. Durable command executor окружает каждый изменяющий intent

Общий application executor сначала durable claim-ит `(caller, idempotency_key)` с HMAC canonical fingerprint и lease, затем выполняет use case и сохраняет serialized safe result. Exact retry воспроизводит result, incompatible payload даёт typed conflict, abandoned claim восстанавливается после lease.

Для provider effects executor передаёт стабильный provider key: registration выводится из registration command identity, charge — из `(auction_id, settlement_round)`, continue/cancel — из attempt identity. При timeout состояние остаётся non-terminal; retry обязан query/reuse ту же provider operation, а не создавать новую.

Альтернатива — in-memory deduplication или idempotency только на стороне Stripe — отклонена: рестарт и сбой после внешнего effect нарушат local replay guarantees.

### 4. Callback — аутентифицированный внешний input и отдельная transaction

HTTP route `/payment-provider-callbacks/stripe` в `src/adapters/inbound/webhook.rs` получает raw body и headers. Stripe adapter проверяет signature и timestamp до deserialization в нейтральный `VerifiedProviderEvent`. Application layer ничего не знает о Stripe event names.

Unit of work одной transaction вставляет inbox row с unique `(provider, event_ref)` и fingerprint, lock-ит aggregate, применяет monotonic transition и пишет outbox. Exact duplicate возвращает success, fingerprint conflict не меняет state. Acknowledgement отправляется только после commit; временная ошибка возвращает retryable HTTP status.

Альтернатива — принять callback через GraphQL/BFF — отклонена: router не должен хранить provider secrets, менять raw body или применять browser auth к server callback.

### 5. Успех имеет приоритет над expiry после commit

Callback success и `ExpirePayment` lock-ят одну attempt row. Если `SUCCEEDED` уже committed, expiry возвращает stable domain outcome. Если expiry получил provider-confirmed cancel/terminal non-success и committed первым, поздний non-authoritative success не может молча переписать состояние: он уходит в reconciliation/security outcome; adapter contract обязан не подтверждать cancel, пока operation потенциально chargeable.

При provider timeout expiry остаётся retryable и не создаёт `PaymentExpired`. Это запрещает Processes переходить к следующему кандидату при неизвестной возможности списания.

Альтернатива — правило last-write-wins — отклонена: оно может породить два settlement candidates с финансовым effect.

### 6. Authz provision и permission checks остаются owner-scoped

Payments adapter использует существующие Authz RPC с bounded deadlines и causal token. Registration проверяет `platform:register_payment_instrument`, provision-ит `payment_profile:owner`; attempt provision-ит owner/process relations и проверяет `payment_attempt:process`, query/continue — `view`/`continue_payment`. Trusted Processes identity выводится только из mesh context.

Grants создаются идемпотентно до local create commit, returned revision сохраняется. Reconciliation удаляет только доказанный orphan после authoritative отсутствия объекта. Authz outage всегда fail closed.

Альтернатива — доверять user/caller id из GraphQL metadata или gRPC payload — отклонена как privilege escalation.

### 7. Контракты additive и provider-neutral

`contexts/payments/proto/gavel/payments/v1/` сохраняет существующие field numbers, добавляет correlation/causal data и typed outcomes только новыми полями/messages. Public event schema/fixtures покрывают readiness и attempt lifecycle; provider refs и recovery URL не входят в events. `contexts/payments/graphql/schema.graphql` сохраняет существующие fields и получает только необходимые safe result/error поля.

Processes client из BSR Generated SDK проверяется in-process E2E, но Temporal orchestration остаётся отдельным будущим change. Auction consumer fixtures остаются source-of-truth для readiness compatibility. Buf и Federation composition являются release gates.

### 8. BSR является versioned distribution seam для RPC contracts

Authoritative source остаётся в owner path `contexts/<context>/proto`, а каждый owner path соответствует отдельному BSR module `buf.build/gavel/<context>`. Для этого change producer — `buf.build/gavel/payments`; уже опубликованный module commit `9604406f76b94df4a5338dba375a302e` и Generated Prost/Tonic SDK подтверждают доступность registry pipeline, но новая schema получает новый commit.

Контракт меняется в две фазы:

1. Owner правит локальную schema и выполняет `buf format`, `buf lint`, `buf build` и breaking check против default-label module в BSR.
2. Workspace публикует schema под label `implement-payments-service`; package names и exact versions разрешаются из BSR SDK metadata/CLI, а не конструируются вручную.
3. `.cargo/config.toml` подключает BSR sparse Cargo registry; developer/CI credentials приходят извне repository.
4. Payments server и Processes consumer закрепляют Generated Prost/Tonic packages точной версией, а `Cargo.lock` фиксирует registry resolution.
5. После contract/release gates проверенный module commit получает release/default label; consumers обновляются явно, без плавающей зависимости.

Payments и мигрируемые consumers удаляют `build.rs`, `protoc-bin-vendored`, `tonic-build` и `tonic::include_proto!`; generated package re-export-ится за context-owned Rust module interface. Checked-in stubs и output `target/buf` не участвуют в production build. `buf.gen.yaml` остаётся только диагностическим local-generation инструментом, пока отдельный repo-wide migration не удалит его; CI проверяет, что Payments build не зависит от его output.

Для contract descriptor/negative tests Buf image берётся из owner source или точного BSR commit, а не генерируется скрытым build script. Недоступность registry/cache при разрешении новой dependency останавливает build: fallback на локальную копию контракта запрещён.

Альтернатива — продолжить компиляцию `.proto` в каждом crate — отклонена: consumers незаметно зависят от working tree producer, требуют локальный Protobuf toolchain и не имеют package-level provenance. Альтернатива — checked-in generated Rust — отклонена из-за drift и шумных code-review diff.

### 9. Stripe — первый adapter, не часть interface домена

`src/adapters/outbound/stripe.rs` реализует provider port `prepare`, `charge`, `continue`, `cancel/query` и webhook verification. Configuration содержит secret references из environment/Secret store; секреты и full provider payload никогда не попадают в errors, traces или metrics.

Adapter contract tests используют локальный deterministic HTTP fake с реальными request/signature/idempotency semantics. Live Stripe access не требуется в CI. Второй adapter позже обязан пройти тот же contract suite, не меняя domain/application interfaces.

Альтернатива — распространить Stripe SDK types через application DTO — отклонена: это сделает смену provider breaking domain change.

### 10. Runtime и эксплуатация повторяют проверенный service shape

`src/bootstrap.rs` поднимает Tokio HTTP GraphQL/webhook/health/readiness/metrics и tonic gRPC, supervises workers и выполняет graceful shutdown. Readiness требует PostgreSQL, Authz и provider configuration; liveness не зависит от downstream. Fatal worker failure снимает readiness и завершает process.

`contexts/payments/ops/` переходит на monorepo multi-stage image, migration Job, минимум три replicas, HTTP/gRPC Services, secretKeyRefs, PDB/topology spread, NetworkPolicy/AuthorizationPolicy, ServiceMonitor, bounded Prometheus rules/dashboard/runbook. Никаких Kubernetes Secret values chart не рендерит.

Metrics используют только bounded operation/outcome/provider-kind/status labels; ids, amounts, emails, URLs, refs, tokens и keys запрещены. Backlog/age gauges покрывают outbox, callbacks requiring reconciliation и stale command claims.

## Risks / Trade-offs

- **[Provider timeout оставляет неопределённый effect]** → стабильный provider key, query-before-retry, non-terminal local state и reconciliation worker; никогда не интерпретировать timeout как decline.
- **[Grant создан, local transaction потеряна]** → идемпотентный provision и conservative orphan reconciliation с causal check.
- **[Callback и command приходят в разном порядке]** → opaque provider ref mapping, row lock, monotonic state machine и inbox receipt в той же transaction.
- **[Partial unique success index создаёт hotspot на auction]** → contention локализован одним settlement auction и является необходимой correctness ценой; измерять conflict rate.
- **[Recovery URL чувствителен]** → хранить только если требуется, возвращать лишь authorized candidate, исключить из events/logs/metrics и соблюдать provider TTL.
- **[Локальный provider fake не моделирует все Stripe edge cases]** → adapter contract fixtures для известных signed events и отдельная staging verification перед production rollout.
- **[BSR или authenticated Cargo index временно недоступен]** → exact versions и `Cargo.lock`, CI cache и отсутствие обновлений dependencies во время runtime; новый build fail closed без подмены contract source.
- **[Preview SDK случайно становится production dependency]** → package version обязана разрешаться к ожидаемому module commit; release gate проверяет label/commit provenance перед promotion.

## Migration Plan

1. Additive расширить owner Protobuf, public event fixtures и Authz schema; проверить Buf/Federation/Auction compatibility и опубликовать Payments module под change label.
2. Разрешить exact Generated Prost/Tonic SDK versions для опубликованного commit, перевести Payments/Processes dependencies и проверить чистую сборку без `protoc`, `build.rs` и `target/buf`.
3. После package provenance/E2E gates продвинуть проверенный BSR commit на release/default label; старые exact SDK versions остаются доступным rollback contract.
4. Развернуть PostgreSQL migration до нового binary; таблицы новые, rollback binary безопасен, пока traffic не направлен.
5. Развернуть Payments с callback route закрытым на edge, проверить readiness, synthetic provider fake и outbox lag.
6. Настроить Stripe endpoint/secret и включить callback traffic; exact duplicate probes должны проходить до GraphQL registration rollout.
7. Включить registration GraphQL, затем Processes owner RPC после проверки dashboards/alerts и contract E2E.
8. При rollback остановить новый ingress/RPC traffic, откатить consumer на прежнюю exact BSR SDK version, сохранить database/outbox/inbox и не удалять provider endpoint до drain подтверждённых callbacks.
