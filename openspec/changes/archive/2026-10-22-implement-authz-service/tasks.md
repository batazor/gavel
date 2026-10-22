# Tasks

## 1. Authz-owned contracts and crate foundation

- [x] 1.1 Additive расширить `gavel.authz.v1` RPC `RegisterPrincipal` и `WriteRelationshipsRequest.deletions`, обновить Auction initializer/contract fixtures и проверить `make buf-check`, `make buf-breaking`, `cargo check -p gavel-auction` без wire-breaking изменений.
- [x] 1.2 Зафиксировать минимальные official `authzed.api.v1` Proto с upstream revision/license/provenance под Authz ownership, подключить generation на Prost/Tonic 0.13 и проверить offline `cargo check -p gavel-authz` плюс Buf format/lint/build.
- [x] 1.3 Подключить Rust 1.85-compatible async/database/crypto/telemetry dependencies, создать `application`, `adapters`, `bootstrap` boundaries и проверить architecture test, запрещающий tonic/sqlx/SpiceDB imports в application core.
- [x] 1.4 Определить стабильные application DTO/error taxonomy и mapping Authz v1 outcomes/gRPC statuses; проверить table-driven tests для invalid, unauthenticated, forbidden, conflict, in-progress, dependency и internal failures.

## 2. Authoritative permission decisions

- [x] 2.1 Реализовать schema-shaped validation resource/principal/permission references и closed permission allowlist; unit tests должны доказать rejection пустых/неизвестных значений до вызова `PolicyEngine`.
- [x] 2.2 Реализовать single `CheckPermission` use case с optional at-least-as-fresh token и fail-closed unknown decision mapping; проверить allow, deny, causal token, malformed token, timeout и transport failure tests.
- [x] 2.3 Реализовать bounded `BulkCheckPermissions` с сохранением input order и общим checked-at token; проверить mixed allow/deny, empty/oversized batch, duplicate checks и невозможность partial response.
- [x] 2.4 Добавить privacy-safe permission tracing/metrics contract одновременно с use cases; проверить автоматическим test отсутствие resource/principal ids, tokens и credentials в logs/labels.

## 3. Scoped relationship provisioning core

- [x] 3.1 Реализовать trusted `WorkloadIdentity` и закрытую workload → resource/relation allowlist для BFF/Auction/Catalog/Payments/Notify/Processes; table tests должны покрыть каждую разрешённую границу и cross-owner denial.
- [x] 3.2 Реализовать canonical principal derivation `u1_` через length-prefixed HMAC-SHA256 и validated HTTPS issuer/subject; проверить determinism, issuer separation, malformed input и отсутствие raw claims в result/serialization.
- [x] 3.3 Определить `CommandReceiptStore` claim/replay/conflict/lease-recovery port и общий mutation executor с keyed fingerprint; in-memory tests должны доказать exact replay, changed payload conflict, concurrent duplicate и recovery только того же fingerprint.
- [x] 3.4 Реализовать `RegisterPrincipal` поверх executor и `platform:gavel#member` TOUCH; проверить BFF-only scope, stable opaque id/result, replay без второго policy call и retryable dependency failure.
- [x] 3.5 Реализовать batch TOUCH/delete use case с canonical ordering, overlap/size validation и atomic `PolicyEngine` mutation; проверить owner scope, delete absent tuple, whole-batch rejection, replay/conflict и safe result.
- [x] 3.6 Добавить provisioning tracing/metrics для claim/replay/conflict/recovery/TOUCH/delete вместе с core; privacy tests должны запретить issuer, subject, tuple ids, idempotency keys и ZedTokens в logs/labels.

## 4. Authz persistence and SpiceDB adapters

- [x] 4.1 Добавить forward-only Authz migration для durable receipts с fingerprint/status/lease/result constraints; disposable PostgreSQL test должен доказать clean apply, repeat run, invalid row rejection и отсутствие raw identity/tuple columns.
- [x] 4.2 Реализовать PostgreSQL receipt claim/finalize/reclaim adapter с transaction/advisory locking; integration tests должны доказать one-winner concurrency, replay, conflict, lease recovery, rollback и safe serialized result.
- [x] 4.3 Реализовать credentialed SpiceDB gRPC adapter для single/bulk checks с connection reuse, deadline и consistency conversion; mock-server tests должны покрыть allow/deny/order/token и fail-closed timeout/unknown responses.
- [x] 4.4 Реализовать один atomic SpiceDB TOUCH/delete batch и readiness probe; tests должны проверить wire mapping, bearer credential injection, duplicate semantic safety и отсутствие call при invalid input/scope.
- [x] 4.5 Добавить pinned disposable PostgreSQL/SpiceDB orchestration и `database-check`, `spicedb-check`, `integration-check` targets в `contexts/authz/Makefile`; проверить отображение через `make help` и отдельный запуск suites одной командой.

## 5. Trusted inbound gRPC boundary

- [x] 5.1 Реализовать tonic `AuthzService` для Check/Bulk/Register/Write с валидацией deadlines и стабильным mapping application outcomes; in-process tests должны проверить каждый success/rejection/retryable path.
- [x] 5.2 Реализовать production workload extractor из sanitized proxy SPIFFE context с trusted-peer check и default-deny test injector; security tests должны доказать, что raw principal/workload metadata не подменяет caller и insecure mode выключен по умолчанию.
- [x] 5.3 Добавить end-to-end gRPC contract tests между generated Authz server/client и существующим Auction adapter; проверить permission allow/deny, relationship write, deletion-compatible request и отсутствие secret/high-cardinality data в errors.

## 6. Runtime, operations and owned documentation

- [x] 6.1 Заменить Authz bootstrap на Tokio HTTP/gRPC runtime с validated config, PostgreSQL/SpiceDB readiness, supervised receipt worker и graceful SIGTERM/SIGINT shutdown; startup tests должны проверить реальные `/healthz`/`/readyz` transitions и worker failure.
- [x] 6.2 Реализовать bounded-label Prometheus rendering и structured tracing wiring для runtime/adapters; snapshot tests должны проверить metric names/outcomes и автоматическую redaction secret/id fields.
- [x] 6.3 Исправить monorepo multi-stage Docker build и обновить Authz Helm для ≥3 replicas default, ports/probes, receipts migration job, separate SpiceDB schema job и external database/SpiceDB/HMAC secret refs; проверить image build, `helm lint` и отсутствие rendered `Secret`.
- [x] 6.4 Обновить Authz Prometheus rules, Grafana dashboard и ops runbook для permission/dependency/receipt SLI и alerts; проверить `make -C contexts/authz observability-check` и dashboard/rule consistency.
- [x] 6.5 Синхронизировать Authz README/ADR/rpc-security, authorize-command Mermaid и `flow.c4`, а также shared deployment model с RegisterPrincipal, receipts datastore, causal checks и workload allowlist; проверить `make arch-validate` и все относительные Authz documentation links.

## 7. Cross-boundary and release verification

- [x] 7.1 Добавить end-to-end scenario `RegisterPrincipal → relationship TOUCH → causal allow → delete → causal deny` и concurrent replay/conflict через real gRPC, PostgreSQL и SpiceDB; проверить receipts, opaque identities, revisions, recovery и owner-scope denial.
- [x] 7.2 Запустить `make -C contexts/authz check`, database/SpiceDB/integration targets, `make -C contexts/auction check`, `make buf-check`, `make buf-breaking`, `make authz-validate`, `make arch-validate` и `make openspec-validate`; зафиксировать все доступные проверки зелёными без secret-like/generated artifacts.

## Workflow follow-up

- После завершения и проверки tracked tasks синхронизировать `authz-permission-decisions` и `authz-relationship-provisioning` в `openspec/specs/` и архивировать change.
- После архивации перейти к Catalog vertical slice; BFF registration/pre-check подключить в BFF-owned change после готовности Authz server.
