# Design

## Context

См. [proposal.md](proposal.md#why). Сейчас `contexts/auction/src/domain/` содержит чистый event-sourced агрегат и 11 unit tests, `src/main.rs` — синхронный hello-world HTTP server, а PostgreSQL migrations и application/adapters отсутствуют. Federation SDL и internal gRPC v1 уже опубликованы; ADR-0001 требует optimistic stream, transactional outbox и Authz, ADR-0002 — version-aware L1/Redis/PostgreSQL read chain. Общий ADR-0005 запрещает CDC приватного event store.

Секретные maximum и bidder identity допустимы только в Auction-owned private stream. Catalog, Payments, Authz и Processes остаются отдельными владельцами; Auction интегрируется через их versioned contracts и собственные проекции.

## Goals / Non-Goals

**Goals:**

- Собрать исполняемый вертикальный срез Auction без инфраструктурных зависимостей в `domain`.
- Сделать один атомарный commit-point для private events, idempotency result, public projection и outbox.
- Обеспечить bounded optimistic retry, at-least-once consumers и безопасные публичные контракты.
- Обслуживать существующие GraphQL/gRPC contracts и version-aware current-price query.
- Сделать инфраструктуру заменяемой in-memory doubles для быстрых application tests и реальными PostgreSQL/Redis/gRPC adapters для integration tests.

**Non-Goals:**

- Реализация бизнес-логики Catalog, Payments, Authz, Processes, Realtime или Analytics.
- Собственный outbox relay: публикацию из PostgreSQL выполняет Debezium согласно ADR-0005.
- Использование Redis как event-store snapshot или источника истины для mutation.
- Выбор окончательного snapshot interval и cache capacity до benchmark; интерфейсы предусматривают конфигурацию.

## Decisions

### 1. Hexagonal module boundary внутри crate Auction

Код разделяется на:

- `src/domain/` — существующий агрегат, value objects и чистые decisions;
- `src/application/` — command/query DTO, use cases, ports и классифицированные application errors;
- `src/adapters/inbound/{graphql,grpc,events}.rs` — преобразование transport identity/input/result;
- `src/adapters/outbound/{postgres,redis,authz}.rs` — реализации ports;
- `src/bootstrap.rs` и `src/main.rs` — config, connection pools, server lifecycle и telemetry.

Application зависит от domain и абстракций ports; adapters зависят внутрь. Transport-specific types не проходят в domain. Альтернатива — поместить SQL и tonic handlers прямо рядом с агрегатом — отклонена: это связывает инварианты с runtime и затрудняет deterministic tests.

### 2. Use case получает request context, а не доверяет payload principal

GraphQL adapter выводит user principal из проверенного JWT context; gRPC adapter выводит service principal из mTLS workload identity. Application вызывает Authz port до загрузки/изменения aggregate и fail-closed классифицирует timeout как retryable dependency failure. Payment readiness читается из Auction-owned projection, обновляемой событиями Payments; payload не может объявить готовность.

После первого принятого bid отдельная идемпотентная relationship-operation provision-ит `auction:participant`; сбой после основного commit попадает в retry/reconciliation queue и не откатывает принятую ставку. До `AuctionOpened` relationship `auction:platform`/`auction:lot` создаётся либо подтверждается; без неё открытие не публикуется.

Альтернатива — доверить проверки Cosmo Router или Processes — отклонена: владельцем команды и последней точки авторизации остаётся Auction.

### 3. Command executor с bounded optimistic retry

Общий executor выполняет: validate idempotency → authorize → load stream/snapshot → fold aggregate → `decide` → atomic commit. PostgreSQL adapter делает conditional append по `expected_version`; unique `(auction_id, version)` является последней защитой. При version conflict executor заново загружает authoritative state и повторяет decision с теми же immutable input/time/idempotency key, максимум конфигурируемое число попыток. Exhaustion возвращает retryable contention.

In-memory implementation того же port моделирует version conflict для deterministic application tests. Per-auction actor/lock не вводится до benchmark: database ordering остаётся корректным базовым механизмом.

### 4. Один service-owned transactional commit port

Application передаёт `CommitRequest`: aggregate id, expected version, private events, safe public events, command fingerprint/result и обновление projection. `AuctionUnitOfWork` атомарно записывает всё либо ничего. Такой coarse-grained port явно выражает consistency boundary и не позволяет use case случайно вызвать event store, outbox и idempotency repository отдельными transactions.

`contexts/auction/migrations/` создаёт:

- `auction_streams(auction_id, current_version, updated_at)`;
- `auction_events(auction_id, version, event_id, event_type, event_schema_version, payload, occurred_at)`;
- `auction_snapshots(auction_id, version, snapshot_schema_version, payload, created_at)`;
- `auction_command_results(principal_scope, idempotency_key, request_fingerprint, result_type, result_payload, stream_version, created_at)`;
- `auction_public_projection(auction_id, lot_id, status, amount_minor, currency_code, ends_at, stream_version, updated_at)`;
- `auction_outbox(event_id, aggregate_type, aggregate_id, aggregate_version, event_type, schema_version, occurred_at, payload)` в форме ADR-0005;
- `auction_inbox(consumer, event_id, payload_fingerprint, processed_at)` и `payment_readiness(user_id, ready, source_version, updated_at)`;
- reconciliation queue для post-commit relationship writes.

SQL constraints обеспечивают уникальность stream version, idempotency scope/key, outbox event id и consumer/event id. Миграции forward-only; rollback релиза выполняется бинарником, совместимым с additive schema.

### 5. Private и public serialization разделены

Private events используют Auction-owned envelope с явными `event_type` и `event_schema_version`; adapter кодирует DTO, а не добавляет serde-зависимость в domain. Upcaster registry восстанавливает старые версии. Snapshot — ускоритель: при неизвестной/повреждённой версии он игнорируется, а aggregate сворачивается из полного stream.

Публичные события получают Protobuf v1 под `contexts/auction/proto/gavel/auction/v1/`; они проходят Buf checks, но регистрируются в Redpanda Schema Registry отдельно от BSR RPC sources. Mapper исчерпывающе преобразует только `PublicEvent`; compile-time API не принимает `AuctionEvent`, поэтому `MaxBidPlaced` нельзя случайно отправить в outbox. Event envelope следует ADR-0005 и несёт aggregate version/correlation metadata без secret fields.

JSONB выбран для private payload и сохранённого result, потому что это внутренние versioned данные одного владельца и упрощает эволюцию/upcasting. Публичный wire остаётся Protobuf. Прямой CDC `auction_events` запрещён конфигурацией connector.

### 6. Idempotency фиксируется вместе с эффектом

Fingerprint строится из operation name, trusted principal scope и канонического application input; секретный maximum может участвовать в keyed digest, но не хранится в telemetry или публичном result. Первая команда резервирует/фиксирует result только внутри той же transaction, что append. Повтор того же fingerprint возвращает сохранённый typed result; другой fingerprint на том же scope/key возвращает conflict.

Для optimistic retry reservation не создаётся отдельно до commit — это исключает зависшие `in_progress` записи. Конкурентный дубль разрешается unique constraint: проигравший читает зафиксированный result.

### 7. Inbound adapters тонкие и используют стабильную error taxonomy

GraphQL resolvers реализуют существующие `auction`, `currentPrice`, `placeBid`, `rescheduleAuction`; бизнес-решения делегируются application. Ожидаемые domain rejections маппятся в стабильные GraphQL extension codes, dependency/contention — в retryable errors. Secret input не логируется.

Tonic `AuctionService` реализует существующий v1 service. Domain rejection возвращается в typed `oneof`; invalid/authentication/authorization/transport failures используют соответствующие gRPC statuses. Deadline propagates в ports. Request idempotency key обязателен. Этот change не меняет published method set.

Event consumers принимают versioned `LotListed`, `PaymentInstrumentReady` и `PaymentInstrumentRevoked`. Inbox insert и изменение локальной projection происходят в одной Auction transaction; дубликат с тем же fingerprint подтверждается, несовпадающий payload для того же event id карантинится и алертит.

### 8. Current-price reader использует монотонную cache chain

`CurrentPriceReader` проверяет bounded L1 с freshness 500 ms. Miss/stale одного auction id объединяется singleflight. Затем reader запрашивает Redis с коротким timeout/circuit breaker и сравнивает version с последней локально увиденной; stale/ошибка ведёт в PostgreSQL projection. Успешный fallback возвращается клиенту независимо от результата opportunistic compare-by-version fill.

Price projector потребляет `PriceChanged` at-least-once и обновляет Redis атомарным Lua compare-by-version. L1 также принимает только не меньшую version. Команды не получают ссылку на cache ports: архитектурная граница не позволяет использовать cache при `decide`.

Альтернатива delete/invalidate cache отклонена: переставленные события могут удалить более новое значение, а miss увеличивает нагрузку на database.

### 9. Async runtime и process topology

Bootstrap использует Tokio. HTTP server обслуживает GraphQL, `/healthz`, `/readyz` и `/metrics`; tonic server слушает отдельный internal port. Background consumers/projector/reconciliation worker запускаются под supervised tasks с cancellation token и graceful shutdown. Readiness требует PostgreSQL и загруженных migrations; Redis и broker degradation отражаются отдельными состояниями, но Redis не делает read API unready. Configuration приходит из environment/secret references и валидируется до bind.

Один binary выбран для первого вертикального среза, чтобы сохранять единый deployment и connection pools. Если профили нагрузки разойдутся, те же application/adapters можно позже запустить отдельными process roles без изменения contracts.

### 10. Observability и privacy

Tracing переносит W3C correlation context через GraphQL, gRPC и broker headers. Metrics включают command latency/outcome, optimistic conflicts/retries, idempotency hits/conflicts, stream replay length, outbox backlog age, consumer lag/duplicates, cache tier hits, singleflight joins, Redis failures и projection age. Labels ограничены operation/outcome/tier; auction/user/idempotency/payment ids и amounts не используются как labels.

Logs структурированы и редактируют bid maximum, JWT, payment ids и raw event payload. Existing Helm ServiceMonitor/PrometheusRule/dashboard расширяются соответствующими SLI/alerts.

### 11. Verification strategy

- Domain tests сохраняются и расширяются property-based проверками proxy/closing/settlement invariants.
- Application contract tests запускаются на in-memory ports: happy paths, authorization failure, idempotent replay/conflict, optimistic retry/exhaustion и atomic failure.
- Adapter tests проверяют private/public codecs, GraphQL/gRPC mapping, secret-field absence и Redis version compare.
- PostgreSQL integration tests применяют migrations к disposable database и проверяют unique constraints/transaction rollback/concurrent append; Redis integration test проверяет compare-by-version.
- Contract checks включают Buf lint/build/breaking, GraphQL composition и fixtures публичных events. Service checks остаются доступны через Auction Makefile.

## Risks / Trade-offs

- **[Risk] JSONB private events требуют дисциплины эволюции** → versioned codec, upcaster tests и fixture compatibility обязательны до изменения schema version.
- **[Risk] Coarse-grained unit-of-work port может быть шире отдельных repositories** → он ограничен одной aggregate transaction; read ports и external side effects остаются отдельными.
- **[Risk] Hot auction создаёт много optimistic retries** → метрики conflict/replay, bounded retries и benchmark определят необходимость actor/partition strategy.
- **[Risk] Post-commit relationship provision временно отстаёт** → durable reconciliation queue, idempotent Authz write и отдельный lag alert; доменный commit не повторяется.
- **[Risk] Redis/projector lag даёт временно старый query** → version в контракте, 500 ms L1 freshness, authoritative fallback и запрет cache в command path.
- **[Risk] Один process содержит servers и workers** → supervised task failure policy, graceful cancellation и возможность выделить roles позже.
- **[Risk] Integration tests зависят от containers** → быстрые in-memory tests остаются обязательными, а repository target поднимает pinned disposable PostgreSQL/Redis для полного CI.

## Migration Plan

1. Добавить additive migrations, public event schemas и codec compatibility tests; применить migrations до нового binary.
2. Развернуть binary с inbound mutations/lifecycle consumers выключенными feature flags; проверить readiness, connection pools и projections.
3. Запустить event consumers с inbox в shadow/replay режиме и сверить проекции, не публикуя новые Auction events.
4. Включить GraphQL commands и internal gRPC для ограниченного traffic; мониторить conflict, idempotency, outbox и Authz SLI.
5. Включить price projector/cache chain, сохраняя PostgreSQL fallback; затем постепенно увеличить cache traffic.
6. Включить Debezium capture только для Auction outbox и проверить safe schema fixtures/consumer lag.

Rollback отключает feature flags и возвращает предыдущий binary; additive tables/columns сохраняются. Уже зафиксированные streams и outbox не удаляются, consumers возобновляют работу по inbox/offset.

## Open Questions

- Snapshot interval, L1 capacity, Redis TTL/eviction и terminal retention выбираются после benchmark; это меняет конфигурацию, но не contracts или task decomposition.
