# Authz

Центральный authorization project на SpiceDB. OAuth/OIDC подтверждает личность; Authz вычисляет fine-grained permissions; доменные проекты после allow всё равно проверяют собственные инварианты.

## ADR

- [0001 — Инициализация проекта](adr/0001-init.md)

## Схема

- [gavel.zed](schema/gavel.zed) — production schema.
- [gavel.validation.yaml](schema/gavel.validation.yaml) — relationships и positive/negative assertions; запускается `zed validate`.
- [Глоссарий](GLOSSARY.md) — canonical language Authz.

## Interface

- [gRPC contract](proto/gavel/authz/v1/authz_service.proto) — внутренние permission checks и caller-scoped relationship writes; правила — в [init ADR](adr/0001-init.md) и [RPC security boundary](rpc-security.md).

Runtime предоставляет `CheckPermission`, `BulkCheckPermissions`, `RegisterPrincipal` и атомарный batch `WriteRelationships` (TOUCH/delete). `RegisterPrincipal` принимает только уже проверенные `iss`/`sub`, возвращает HMAC-derived opaque `u1_…` principal и создаёт `platform:gavel#member`. Каждый mutation сначала фиксируется в PostgreSQL receipt store; точный повтор возвращает сохранённый результат, а тот же key с другим canonical payload даёт conflict.

## Юзкейсы

- [authorize-command](use-cases/authorize-command/README.md) — pre-check в BFF и authoritative check у владельца команды

## Enforcement

- После первого verified login edge/subgraph идемпотентно вызывает Authz `RegisterPrincipal`; это создаёт `platform:member` без отдельного user profile/database.
- Cosmo или owner subgraph может сделать pre-check/BulkCheck для быстрого GraphQL error и viewer capabilities.
- Catalog, Auction, Payments и Notify являются authoritative policy enforcement points для своих команд.
- Processes действует как `service_account:processes`; пользовательский principal нельзя подменить service identity.
- Публичные GraphQL fields (список лотов, публичная цена, bid history и auction updates) не требуют authorization decision, потому что не раскрывают приватные поля.
- При недоступности SpiceDB mutations и private reads fail closed; публичные reads продолжают работать.

## Permission contract

| Resource | Permissions | Principal source |
|---|---|---|
| `platform:gavel` | `create_lot`, `register_payment_instrument` | Gavel member |
| `lot:{lotId}` | `is_seller`; `edit`, `withdraw`, `reschedule_auction`, `view_seller_state`; `execute_withdraw` | seller identity; seller/admin management; process worker saga execution |
| `auction:{lotId}` | `place_bid`, `view_personal_state`, `reschedule` | member, participant, seller according to permission |
| `auction:{lotId}` | `close`, `cancel`, `reopen`, `settle` | `service_account:processes` only |
| `payment_profile:{principalId}` | `manage` | owner only |
| `payment_attempt:{attemptId}` | `view`, `continue_payment` | owner only |
| `payment_attempt:{attemptId}` | `process` | `service_account:processes` only |
| `notification:{notificationId}` | `view` / `deliver` | recipient / process worker |
| `operation:{operationId}` | `view` | owner only |

`place_bid` разрешает member вызвать команду, но не утверждает, что ставка допустима: payment readiness, дедлайн и сумма остаются проверками Auction.

## Кто пишет relationships

| Owner | Relationships |
|---|---|
| Authz/bootstrap | `platform:member`, `platform:administrator`, `platform:process_worker` |
| Catalog | `lot:platform`, `lot:seller` |
| Auction | `auction:platform`, `auction:lot`, `auction:participant` |
| Payments | `payment_profile:owner`, `payment_attempt:owner/platform/auction` |
| Notify | `notification:recipient/platform` |
| BFF | `operation:owner` |

Relationship writes use stable domain ids and are idempotent. Они проходят через Authz relationship writer, который по mTLS workload identity разрешает каждому проекту только его resource types. The owning context stores the latest ZedToken needed for causal permission checks.

Закрытая workload allowlist: BFF регистрирует principals и владеет `operation`; Catalog — `lot`; Auction — `auction`; Payments — `payment_profile`/`payment_attempt`; Notify — `notification`; Processes может только читать permissions. Любой неизвестный workload или cross-owner write отклоняется до SpiceDB.

## Operations

- [Tokio runtime](src/main.rs) — HTTP health/metrics, internal gRPC, readiness PostgreSQL/SpiceDB и graceful shutdown.
- [Docker image, Helm chart, Prometheus rules и Grafana dashboard](ops/Helm/README.md)
