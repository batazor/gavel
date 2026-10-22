# Tasks

## 1. Auction-owned contracts and crate foundation

- [x] 1.1 Добавить versioned Protobuf v1 для безопасных Auction public events и fixtures, доказать отсутствия `maximum`, `user_id`, private leader/storage keys через contract tests и `make buf-check`.
- [x] 1.2 Подключить совместимые с Rust 1.85 async/web/database/cache/serialization/telemetry dependencies, создать `application`, `adapters` и `bootstrap` module boundaries и проверить `cargo check -p gavel-auction` и запрет infrastructure imports в `src/domain/`.
- [x] 1.3 Зафиксировать стабильные application DTO/error taxonomy и маппинг существующих GraphQL/gRPC v1 outcomes; проверить table-driven tests и `make graphql-check`/`make buf-check` без breaking contract change.

## 2. Auction application core

- [x] 2.1 Определить ports `AuctionUnitOfWork`, `Authorization`, `PaymentReadiness`, public projection/cache, inbox и reconciliation с in-memory implementations; проверить contract tests для atomic commit/rollback и dependency failures.
- [x] 2.2 Реализовать общий command executor с trusted request context, idempotency fingerprint/result и bounded optimistic retry; проверить replay, key conflict, concurrent duplicate, version retry и retry exhaustion в application tests.
- [x] 2.3 Реализовать use case открытия из `LotListed` и post-commit Authz relationship reconciliation; проверить first delivery, exact duplicate, payload conflict и retry queue tests.
- [x] 2.4 Реализовать `placeBid` с permission/payment-readiness checks и безопасным result/outbox mapping; проверить happy path, immediate outbid, forbidden, Authz timeout, payment required и отсутствие secret fields.
- [x] 2.5 Реализовать `rescheduleAuction`; проверить seller permission, future time, closed state, policy restrictions, idempotent replay и `AuctionRescheduled` outbox tests.
- [x] 2.6 Реализовать lifecycle use cases close/cancel/forfeit/confirm поверх общего executor; проверить typed domain outcomes, stale round/candidate, terminal replay и отсутствие provider-specific payment data.

## 3. Auction PostgreSQL ownership and codecs

- [x] 3.1 Добавить forward-only migrations для streams, private events, snapshots, command results, public projection, outbox, inbox, payment readiness и reconciliation queue; проверить применение с нуля, повторный запуск и SQL constraints на disposable PostgreSQL.
- [x] 3.2 Реализовать versioned private-event codec/upcasters и snapshot codec вне domain; проверить round-trip всех событий, historical fixtures и fallback при неизвестном/повреждённом snapshot.
- [x] 3.3 Реализовать PostgreSQL load/fold и atomic `AuctionUnitOfWork` с conditional append; integration tests должны доказать rollback all-or-nothing, unique stream versions, idempotent winner-read и concurrent append conflict.
- [x] 3.4 Реализовать inbox/payment-readiness consumers storage и reconciliation repository; integration tests должны доказать at-least-once duplicate safety, source-version monotonicity и quarantine payload conflict.
- [x] 3.5 Добавить database integration targets и pinned disposable PostgreSQL orchestration в `contexts/auction/Makefile`; проверить, что `make help` показывает цели и database suite проходит одной командой.

## 4. Security and inbound adapters

- [x] 4.1 Реализовать Authz gRPC adapter с deadlines, fail-closed classification и idempotent relationship writes; проверить allowed/denied/timeout/transport mappings mock-server tests и `make authz-validate`.
- [x] 4.2 Реализовать GraphQL resolvers `auction`, `currentPrice`, `placeBid`, `rescheduleAuction` с principal из verified context и стабильными extension codes; выполнить schema/operation tests, secret-redaction tests и `make graphql-check`.
- [x] 4.3 Реализовать tonic `AuctionService` для четырёх существующих RPC, identity/deadline extraction и typed outcomes; выполнить in-process gRPC contract tests для auth, idempotency, domain rejection и retryable status.
- [x] 4.4 Реализовать versioned inbound consumers `LotListed`, `PaymentInstrumentReady` и `PaymentInstrumentRevoked` с inbox/quarantine semantics; проверить fixtures владельцев, duplicates, out-of-order source versions и malformed payload handling.

## 5. Public current-price path

- [x] 5.1 Реализовать authoritative PostgreSQL current-price projection/read port, обновляемый в command transaction; проверить version/status/terminal snapshots и доказать тестом, что command executor не обращается к cache ports.
- [x] 5.2 Реализовать bounded L1 с 500 ms freshness, monotonic compare и per-auction singleflight; проверить fresh/stale behavior, capacity eviction и один downstream call при concurrent refresh.
- [x] 5.3 Реализовать Redis L2 adapter с timeout/circuit breaker и atomic compare-by-version, плюс `PriceChanged` projector; проверить duplicate/out-of-order events и Lua integration tests на disposable Redis.
- [x] 5.4 Собрать `CurrentPriceReader` L1 → Redis → PostgreSQL с best-effort fills; проверить fallback при miss/timeout/write failure, not-found и невозможность старой версии затереть новую.
- [x] 5.5 Добавить cache/projector integration target в Auction Makefile и cache privacy fixtures; проверить `make help` и полный Redis suite одной командой.

## 6. Runtime, operations and owned documentation

- [x] 6.1 Заменить bootstrap-заглушку на Tokio runtime с HTTP/GraphQL, internal gRPC, supervised consumers/projector/reconciliation, validated config и graceful shutdown; проверить startup/shutdown tests и реальные `/healthz`/`/readyz` semantics.
- [x] 6.2 Подключить structured tracing и bounded-label metrics для commands, retries, idempotency, replay, outbox/inbox, consumers и cache; проверить metric names/labels и автоматический тест, что secrets/identifiers не попадают в logs или labels.
- [x] 6.3 Обновить Auction Helm values/templates для database migrations, runtime flags, ports, probes и external secret references; проверить `helm lint contexts/auction/ops/Helm` и отсутствие materialized secrets/generated manifests.
- [x] 6.4 Обновить Prometheus rules, Grafana dashboard и ops README для новых SLI/alerts; проверить `make -C contexts/auction observability-check`.
- [x] 6.5 Синхронизировать Auction README/ADR, use-case Mermaid и `flow.c4` для place-bid, current-price, reschedule и close-by-deadline с реализованными adapters/transactions/failures; проверить `make arch-validate` и ссылки документации.

## 7. Cross-contract and release verification

- [x] 7.1 Добавить end-to-end integration scenarios `LotListed → open → placeBid → close → forfeit/confirm`, включая duplicate delivery и concurrent bid; проверить streams, projections, idempotency и safe outbox в disposable PostgreSQL/Redis environment.
- [x] 7.2 Запустить `make -C contexts/auction check`, database/cache/integration targets, `make buf-check`, `make buf-breaking`, `make graphql-check`, `make authz-validate`, `make arch-validate` и `make openspec-validate`; зафиксировать все доступные проверки зелёными без secret-like/generated artifacts в repository.

## Workflow follow-up

- После завершения и проверки всех tracked tasks синхронизировать три capability specs в `openspec/specs/` и архивировать change.
- После архивации перейти к следующему bounded-context vertical slice общего DDD roadmap.
