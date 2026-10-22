# Tasks

## 1. Catalog contracts and crate foundation

- [x] 1.1 Проверить/additive расширить Catalog GraphQL SDL и `gavel.catalog.v1` для correlation/consistency данных без изменения существующих field numbers; обновить generated initializers и проверить `make buf-check`, `make buf-breaking`, `make -C contexts/bff graphql-check`.
- [x] 1.2 Зафиксировать Catalog-owned `LotListed`, `LotUpdated`, `LotWithdrawn` v1 envelope/fixtures и проверить contract tests, что существующий Auction consumer принимает `LotListed` без изменений и public payload не содержит object keys/Authz tokens.
- [x] 1.3 Подключить Rust 1.85-compatible async/GraphQL/PostgreSQL/crypto/S3 dependencies, создать `domain`, `application`, `adapters`, `bootstrap` boundaries и проверить architecture test без tonic/sqlx/GraphQL/S3 imports в domain/application core.
- [x] 1.4 Определить transport-independent DTO, stable application error taxonomy и GraphQL/gRPC mapping; table tests должны покрыть invalid, unauthenticated, forbidden, conflict, in-progress, dependency и internal outcomes без unsafe error values.

## 2. Catalog domain model

- [x] 2.1 Реализовать typed ids, bounded title/description/alt, money/currency, bid step и closing-policy value objects; property/table tests должны покрыть границы, normalization и invalid construction.
- [x] 2.2 Реализовать aggregate `Lot` с `LISTED/WITHDRAWN`, version, causal Authz revision и monotonic description freeze; domain tests должны покрыть create/update/freeze/withdraw, forbidden transitions и отсутствие торговой логики.
- [x] 2.3 Реализовать `LotImage` state machine `PENDING → READY → ATTACHED` и invalid/expired cleanup states; tests должны доказать owner isolation, metadata match, single attachment и невозможность автоматического удаления ATTACHED.
- [x] 2.4 Реализовать доменную проверку 1–20 unique ordered images и атомарное решение attach при создании лота; tests должны отклонять empty/oversized/duplicate/foreign/non-READY batch целиком.

## 3. Media application capabilities

- [x] 3.1 Определить ports `ObjectStorage`, media repository/UoW, clock/id generation и lifecycle leases без SDK types; fake-port tests должны доказать deterministic timeout/error mapping.
- [x] 3.2 Реализовать идемпотентный `RequestLotImageUpload`: validate → durable PENDING intent → bounded presign; tests должны покрыть exact replay, changed-payload conflict, presign failure recovery и safe response без object key/credentials.
- [x] 3.3 Реализовать `CompleteLotImageUpload` с HEAD size/type/checksum verification и atomic READY transition; tests должны покрыть success/replay, missing/mismatch/expired/foreign image и отсутствие READY при dependency failure.
- [x] 3.4 Реализовать lease-based cleanup/reconciliation expired PENDING, unattached READY и failed verification; tests должны покрыть concurrent workers, idempotent delete/not-found, retryable storage failure и защиту ATTACHED rows.
- [x] 3.5 Добавить media telemetry/redaction одновременно с use cases; snapshot tests должны запретить seller/image ids, object keys, URLs, checksums и credentials в labels/logs.
- [x] 3.6 Синхронизировать `upload-lot-image` README Mermaid и `flow.c4` с реализованными replay/failure/reconciliation paths и проверить `make arch-validate` плюс относительные Catalog links.

## 4. Lot lifecycle and authorization application

- [x] 4.1 Реализовать общий Catalog command receipt/executor с HMAC canonical fingerprint, claim/replay/conflict и optimistic retry contract; concurrent in-memory tests должны доказать one-effect semantics и safe serialized results.
- [x] 4.2 Реализовать `ListLot`: validate/claim → `platform:create_lot` check → Authz TOUCH `lot:platform/seller` → local UoW; tests должны покрыть deny/fail-closed, owner-scoped batch, exact replay, ambiguity-window retry и no commit before revision.
- [x] 4.3 Реализовать `UpdateLot` с `lot:edit`, expected version и freeze; tests должны покрыть successful update/outbox, non-owner deny, post-bid conflict, concurrent version conflict и replay.
- [x] 4.4 Реализовать Processes-only `WithdrawLot` с `lot:execute_withdraw`; tests должны покрыть listed→withdrawn, already-withdrawn replay, caller denial, Authz unavailable, conflict и ровно один `LotWithdrawn`.
- [x] 4.5 Реализовать monotonic Auction event/inbox handler для первого принятого bid/freeze без synchronous Auction query; fixture tests должны покрыть duplicate, stale, changed-payload quarantine и неизвестную schema version.
- [x] 4.6 Реализовать Authz-grant reconciliation после незафиксированного local create; tests должны доказать delayed lease, proof-of-absence guard, safe TOUCH replay/delete и отсутствие удаления grant существующего lot.
- [x] 4.7 Добавить lifecycle/outbox/reconciliation telemetry и privacy tests; обновить `list-lot`/`withdraw-lot` README Mermaid и `flow.c4`, затем проверить `make arch-validate`.

## 5. PostgreSQL persistence and event outbox

- [x] 5.1 Добавить forward-only migrations для lots, images, receipts, inbox, outbox и reconciliation leases с FK/check/unique constraints; disposable PostgreSQL tests должны доказать clean/repeat apply и invalid-row rejection.
- [x] 5.2 Реализовать SQLx Catalog UoW, receipt locking и optimistic version commit; integration tests должны доказать atomic create/attachments/result/outbox, rollback, replay/conflict и one-winner concurrency.
- [x] 5.3 Реализовать PostgreSQL public reader с deterministic `(listed_at, lot_id)` pagination и HMAC cursor validation; tests должны покрыть stable pages, inserts between pages, malformed/tampered cursor, limit и visibility states.
- [x] 5.4 Реализовать inbox/freeze, outbox leasing/retention и media/Authz reconciliation repositories; integration tests должны доказать monotonic/idempotent claims, crash recovery и запрет удаления unpublished/PENDING work.
- [x] 5.5 Добавить `database-check` target с pinned disposable PostgreSQL и отобразить его через `make -C contexts/catalog help`; отдельный запуск всех persistence suites должен быть зелёным.

## 6. Inbound GraphQL and gRPC adapters

- [x] 6.1 Реализовать Federation queries `lot/lots` и delivery-safe image mapping поверх application reader; executable-schema tests должны проверить exact SDL ownership, cursor pages, null hidden lot и отсутствие private fields.
- [x] 6.2 Реализовать GraphQL media/list/update mutations с verified principal extension и stable error extensions; in-process tests должны покрыть success/rejection/retryability и доказать игнорирование raw principal headers/arguments.
- [x] 6.3 Реализовать tonic `CatalogService.WithdrawLot` с mandatory bounded deadline и application outcome mapping; in-process tests должны покрыть success/replay/domain rejection/dependency/invalid paths.
- [x] 6.4 Реализовать production trusted-proxy SPIFFE extractor только для `gavel-processes` и fail-closed test injector; security tests должны отклонять untrusted peer, spoofed metadata и insecure default.
- [x] 6.5 Добавить generated Processes/Catalog gRPC contract E2E и GraphQL composition check; проверить idempotency key, correlation/deadline, typed outcome и отсутствие secret/high-cardinality data в errors.

## 7. Outbound Authz and S3-compatible adapters

- [x] 7.1 Реализовать Catalog Authz gRPC adapter для `create_lot`, `edit`, `execute_withdraw` и atomic `lot:platform/seller` TOUCH/delete с deadlines/causal token; mock/in-process tests должны покрыть allow/deny/order/token и fail-closed failures.
- [x] 7.2 Реализовать S3-compatible presign PUT/HEAD/delete adapter с endpoint/path-style configuration, required checksum headers и connection reuse; MinIO tests должны покрыть successful upload, metadata match, not-found, mismatch, expiry и credential redaction.
- [x] 7.3 Добавить pinned disposable MinIO orchestration и `storage-check`/`integration-check` targets; real boundary flow upload → complete → ListLot → query → cleanup должен быть зелёным с PostgreSQL и fake/real Authz по назначению.
- [x] 7.4 Добавить cross-context E2E: real Catalog create/outbox fixture → существующий Auction `LotListed` consumer открывает ровно один auction при duplicate delivery; проверить ids, closing policy и public privacy.

## 8. Runtime, operations and owned documentation

- [x] 8.1 Заменить bootstrap на Tokio HTTP GraphQL/health/metrics + gRPC runtime с validated config, PostgreSQL/Authz/object-storage readiness, supervised cleanup/reconciliation workers и graceful shutdown; startup tests должны проверить transitions/fatal worker failure.
- [x] 8.2 Реализовать bounded Prometheus rendering и structured tracing wiring для commands/queries/media/dependencies/outbox/workers; snapshot tests должны проверить names/outcomes и автоматическую redaction ids/keys/URLs/tokens/credentials.
- [x] 8.3 Исправить monorepo multi-stage Docker build и обновить Catalog Helm для ≥3 replicas, HTTP/gRPC ports/probes, migration Job и external database/Authz/S3/HMAC secret refs; проверить image build, `helm lint` и отсутствие rendered `Secret`.
- [x] 8.4 Обновить Catalog Prometheus rules, Grafana dashboard и ops runbook для GraphQL/RPC latency, dependency failures, pending-media age, outbox backlog и reconciliation; проверить `make -C contexts/catalog observability-check` и contract consistency.
- [x] 8.5 Синхронизировать Catalog README/ADRs/rpc-security, все use-case Mermaid/`flow.c4` и shared deployment model с implemented boundaries; проверить `make arch-validate` и все относительные Catalog documentation links.

## 9. Release verification

- [x] 9.1 Запустить `make -C contexts/catalog check`, database/storage/integration targets, `make -C contexts/auction check`, `make buf-check`, `make buf-breaking`, `make graphql-check`, `make arch-validate`, `make helm-check` для Catalog и `make openspec-validate`; зафиксировать все доступные проверки зелёными без generated/secret-like artifacts.

## Workflow follow-up

- После завершения синхронизировать `catalog-media-management`, `catalog-lot-lifecycle`, `catalog-lot-discovery` и `internal-rpc-contracts`, затем архивировать change.
- После архивации перейти к Payments vertical slice; Processes/Temporal orchestration реализовать после готовности Catalog и Payments owner RPC.
