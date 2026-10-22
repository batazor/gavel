# Design

## Context

См. `proposal.md` — Why. `contexts/catalog/` уже владеет Federation SDL, `gavel.catalog.v1.WithdrawLot`, use-case docs и `LotListed v1` fixture, но Rust crate экспортирует только generated RPC types, а binary обслуживает hello-world HTTP. Auction уже строго декодирует `LotListed v1`, поэтому producer должен сохранить envelope и payload names. Authz предоставляет готовые causal checks, scoped relationship writes и durable replay своей стороны.

Catalog хранит обычное aggregate state, media metadata, idempotency receipts и outbox в собственной PostgreSQL. Между catalog-db, Authz и S3-compatible storage нет общей транзакции. Domain не должен зависеть от async runtime, GraphQL, tonic, SQLx, S3 SDK или generated contracts.

## Goals / Non-Goals

**Goals:**

- построить тестируемые domain/application boundaries для полного Catalog MVP;
- обеспечить атомарность внутри catalog-db и детерминированное recovery на внешних seams Authz/S3;
- сохранить совместимость публичного SDL, Catalog RPC и `LotListed v1` consumer;
- дать production runtime проверяемые readiness, telemetry, migrations и deployment artifacts.

**Non-Goals:**

- вычислять auction price, bid presence, deadline или право компенсации;
- реализовывать Processes/Temporal workflow либо Redpanda/Debezium runtime;
- проксировать image bytes, преобразовывать изображения или выбирать production CDN/provider;
- добавлять распределённые транзакции или общий datastore между contexts.

## Decisions

### 1. Domain и application разделены от adapters

`contexts/catalog/src/domain/` содержит `Lot`, `LotImage`, typed ids, money/closing inputs, states и чистые decisions. `src/application/` содержит commands/queries, error taxonomy и ports: `CatalogUnitOfWork`, `CatalogReader`, `Authorization`, `ObjectStorage`, `Clock`, `IdGenerator`, `MediaCleanupQueue`. Imports tonic/SQLx/GraphQL/S3 запрещаются architecture test.

Application command executor canonicalize-ит payload, выполняет authorization и оркестрирует один unit of work. Альтернатива с правилами в resolver/gRPC handler отклонена: GraphQL и Processes получили бы разные invariants и replay semantics.

### 2. Lot — optimistic aggregate, image — Catalog-owned entity

`Lot` имеет version и состояния `LISTED | WITHDRAWN`; create начинает с version 1, update/withdraw требуют expected current version. `LotImage` имеет `PENDING | READY | ATTACHED`, owner, opaque object key, expected metadata, expiry и optional lot/order/alt. Domain проверяет 1–20 unique images, bounded text, positive money/bid step, ISO currency и closing-policy shape.

Freeze после первой ставки хранится monotonic flag в lot row и применяется из versioned Auction event/inbox отдельным adapter. Это не попытка Catalog определить число ставок. Альтернатива с синхронным Auction query на каждый edit отклонена из-за coupling и гонки после ответа.

### 3. Все локальные эффекты фиксирует один PostgreSQL unit of work

Forward-only migrations создают `catalog_lots`, `catalog_lot_images`, `catalog_command_receipts`, `catalog_outbox`, `catalog_inbox` и reconciliation queue с constraints. Для command transaction выполняется advisory/row lock по aggregate/key, fingerprint conflict detection, state transition, attachments, safe serialized result и outbox insert.

Exact replay читается до authorization/external calls, если result уже terminal. Другой payload для того же caller/key — conflict. Optimistic version conflict retryable и не оставляет partial rows. Outbox envelope содержит UUID event id, aggregate version, occurred-at millis, correlation/causation и schema version.

### 4. Upload — saga PENDING → READY без distributed transaction

`RequestLotImageUpload` сначала durable сохраняет PENDING intent с opaque generated object key, затем получает presigned PUT из `ObjectStorage`. Если presign не удался, intent остаётся безопасным для retry/cleanup; exact retry использует тот же key и object.

`CompleteLotImageUpload` получает HEAD metadata с bounded timeout, сравнивает size/type/checksum constant-time where applicable и transactionally отмечает READY. Mismatch/expired intent queue-ит delete. Cleanup выбирает только expired non-ATTACHED rows lease-based batch, идемпотентно вызывает delete и затем удаляет/терминализирует metadata. SDK-specific types не выходят из adapter.

### 5. Create использует claim → Authz → local commit

Порядок `ListLot`:

1. validate и claim Catalog command receipt;
2. authoritative `platform:gavel#create_lot` check;
3. одним Authz batch TOUCH создать `lot:<id>#platform@platform:gavel` и `lot:<id>#seller@user:<principal>` с производным stable idempotency key;
4. одной catalog-db transaction подтвердить READY ownership, создать lot, ATTACHED images, сохранить ZedToken/result и `LotListed`.

Повтор после шага 3 безопасен: Authz replay идемпотентен. Queue reconciliation хранит только opaque ids/fingerprint-safe fields и удаляет grant, когда отсутствие lot доказано после bounded lease. Компенсация до истечения ambiguity window запрещена. Альтернатива local commit до grants отклонена: публичный лот мог бы существовать без owner permissions.

### 6. Event contract кодируется Catalog-owned adapter

`src/adapters/outbound/events.rs` создаёт strict camelCase JSON `LotListed v1`, полностью совместимый с `contexts/catalog/tests/fixtures/lot_listed_v1.json` и Auction decoder: `auctionId == lotId` для MVP, seller/start price/currency/bid step/closing policy. `LotUpdated` и `LotWithdrawn` получают собственные v1 fixtures.

DB trigger/relay не сериализует domain objects: transaction пишет готовый public envelope, Debezium публикует row. Contract tests декодируют producer fixture существующим Auction consumer. Private metadata, object keys, Authz token и receipt payload не попадают в public event.

### 7. GraphQL subgraph является тонким inbound adapter

`src/adapters/inbound/graphql.rs` реализует существующие queries/mutations через async-graphql Federation. Verified principal берётся из trusted request extension, не из argument/header. Money остаётся string на GraphQL boundary, внутри преобразуется в minor units по закрытой currency scale policy; timestamps — RFC3339 на wire и millis value objects внутри.

Cursor — versioned HMAC-authenticated encoding `(listed_at, lot_id)`, `first` ограничен безопасным максимумом. Query возвращает только `LISTED` rows и delivery URLs, построенные adapter без object key. Domain rejection получает stable GraphQL extensions; dependency failures retryable. Router только композит/transport.

### 8. Catalog gRPC доверяет только Processes workload

`src/adapters/inbound/grpc.rs` реализует `WithdrawLot` с обязательным bounded deadline и stable mapping. Interceptor принимает sanitized SPIFFE identity только от loopback/configured trusted proxy CIDRs; raw workload/principal metadata игнорируются. Единственный mutation caller — `gavel-processes`.

Handler проверяет `lot#execute_withdraw` как `service_account:processes`, затем вызывает application command. Success/rejection остаются в oneof; missing identity/deadline и retryable dependency failures — gRPC status. Proto меняется только additive при необходимости correlation/consistency fields; Buf доказывает compatibility.

### 9. PostgreSQL, Authz и object storage adapters имеют bounded timeouts

SQLx pool принадлежит runtime. Authz использует generated `gavel-authz` client через `Authorization` port; single check, relationship batch и returned revision маппятся fail closed. S3-compatible adapter использует AWS SigV4-compatible client только для presign PUT, HEAD и delete; endpoint/path-style/config поступают извне.

Disposable integration поднимает pinned PostgreSQL и MinIO. Authz contract покрывается in-process fake для большинства tests и real Authz gRPC E2E для create/provisioning boundary. Credentials никогда не имеют default production values.

### 10. Runtime, observability и rollout принадлежат Catalog

Tokio runtime слушает HTTP GraphQL/health/metrics и отдельный gRPC port, запускает cleanup/reconciliation workers и graceful shutdown. `/readyz` требует PostgreSQL, object-storage probe и Authz readiness; worker fatal failure снимает readiness.

Metrics используют bounded dimensions: command/query/media operation, outcome, dependency и outbox/reconciliation state. IDs, cursors, keys, URLs, checksums, tokens и credentials запрещены tests. Catalog rules/dashboard/runbook покрывают GraphQL/RPC latency, dependency failure, pending media age, outbox backlog и reconciliation failures.

Docker build использует monorepo root/workspace lock. Helm default — ≥3 replicas, separate SQL migration Job, external database/Authz/S3/HMAC secret refs, probes, ports и no rendered Secret. Schema/event migrations deploy-ятся до consumers; application rollout остаётся backward compatible.

## Risks / Trade-offs

- **[Authz write выполнен, catalog commit нет]** → stable ids/keys, delayed reconciliation и exact retry исключают публичный lot без grants и опасную раннюю компенсацию.
- **[S3 object и metadata расходятся]** → expected metadata, HEAD verification, explicit lifecycle и idempotent cleanup; ATTACHED никогда не удаляется автоматически.
- **[Freeze event задержан]** → Catalog может кратко принять edit рядом с первой ставкой; Auction facts применяются monotonically, а изменение описания не влияет на торговые инварианты. При необходимости строгий barrier станет отдельным cross-context protocol.
- **[Cursor становится forgeable/high-cardinality]** → HMAC versioning, bounded fields и запрет cursor в telemetry.
- **[Outbox растёт]** → relay marks delivery and retention удаляет только terminal rows после replay window; unpublished rows не удаляются.
- **[SDK увеличивает binary/build]** → S3 client ограничен adapter; domain/application и contract tests не зависят от provider implementation.

## Migration Plan

1. Добавить additive contracts/fixtures и проверить Buf, GraphQL composition и Auction consumer до Catalog rollout.
2. Применить Catalog SQL migrations и external secret/config references; это не меняет существующие rows, потому что production data ещё нет.
3. Развернуть runtime с workers и mutations выключенными config gate; проверить readiness, queries и telemetry.
4. Включить media upload/cleanup, затем create/update после проверки Authz provisioning и outbox delivery.
5. Включить Processes-only `WithdrawLot` после consumer/client contract verification; сама Temporal saga остаётся отдельным rollout.
6. Rollback binary сохраняет forward-only tables/outbox. Созданные Authz grants удаляются только reconciliation/explicit delete; опубликованные events не откатываются SQL rollback.
