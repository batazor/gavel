# Proposal

## Why

Authz уже владеет SpiceDB schema и versioned gRPC contract, но исполняемый сервис остаётся hello-world scaffold: authoritative checks, principal registration и scoped relationship writes фактически отсутствуют. Это блокирует безопасную реализацию остальных bounded contexts, потому что их protected commands должны fail closed через единый policy decision point.

## What Changes

- Реализовать Authz application core с валидируемыми DTO, стабильной error taxonomy, single/bulk permission decisions и явной consistency policy по ZedToken.
- Подключить официальный versioned SpiceDB gRPC API через outbound adapter с bounded deadlines, connection reuse и fail-closed mapping.
- Расширить `gavel.authz.v1` additive RPC `RegisterPrincipal` и additive deletion input для relationship writer; сохранить wire compatibility существующих v1 clients.
- Реализовать стабильный неперсональный principal id из verified `(issuer, subject)` и идемпотентное создание `platform:gavel#member` только для доверенного BFF workload.
- Ограничить relationship mutations allowlist-матрицей workload → resource/relation, поддержать TOUCH/delete, durable idempotency fingerprint/result и возврат ZedToken.
- Добавить Authz-owned PostgreSQL receipts/migrations, не смешивая их с отдельным datastore SpiceDB; проверить атомарность и конфликт повторного idempotency key.
- Заменить bootstrap на Tokio HTTP/gRPC runtime с trusted workload metadata, readiness зависимости, graceful shutdown, bounded-label telemetry и production Helm/configuration.
- Добавить disposable PostgreSQL + SpiceDB integration tests, contract/security tests и синхронизировать Authz README/ADR/Mermaid/LikeC4/operations.

## Capabilities

### New Capabilities

- `authz-permission-decisions`: authoritative single/bulk permission checks, consistency tokens, fail-closed errors и bounded request behavior.
- `authz-relationship-provisioning`: verified principal registration, caller-scoped relationship writes/deletes, durable idempotency и causal ZedToken results.

### Modified Capabilities

Нет. Additive Authz v1 contract остаётся в рамках существующих требований `internal-rpc-contracts` и не меняет их нормативное поведение.

## Impact

- **Primary owner:** `contexts/authz/` — Proto contract, application/adapters/runtime, migrations, tests, Docker/Helm/observability и owned documentation.
- **Impacted consumer:** `contexts/auction/` — регенерация Authz client и явное пустое deletion-поле в существующем write request; доменные правила Auction и wire behavior не меняются.
- **Shared contracts/docs:** root Buf workspace и `docs/architecture/` только при необходимости отразить отдельный Authz receipts datastore и новые RPC flows.
- **Dependencies/systems:** официальный SpiceDB gRPC API, Authz-owned PostgreSQL receipts store, существующие SpiceDB schema migration/validation и external secret references.
- **Compatibility:** Proto изменения только additive; `buf breaking` должен оставаться зелёным. Rust consumers могут потребовать механическое обновление generated struct initializers.
- **Out of scope:** OAuth/OIDC login UI, проверка JWT на public edge, BFF pre-check integration, реализация Catalog/Payments/Notify clients, domain eligibility, production credentials и фактическое production deployment.
