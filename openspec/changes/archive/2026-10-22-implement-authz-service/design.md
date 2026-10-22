# Design

## Context

См. `proposal.md` — Why. Сейчас `contexts/authz/` содержит принятую SpiceDB schema, assertions и `gavel.authz.v1`, но library экспортирует только generated RPC types, а binary обслуживает синхронный hello-world HTTP. Auction уже использует Authz client с deadline/fail-closed mapping, поэтому новый server обязан сохранить существующий wire contract и не переносить domain eligibility в policy layer.

Workspace закреплён на Rust 1.85, Prost 0.13 и Tonic 0.13. Доступные сторонние Rust bindings SpiceDB используют другую Tonic/Rust baseline; обновление всего workspace ради одного adapter расширило бы change на восемь crates. Raw SpiceDB credentials должен иметь только Authz.

## Goals / Non-Goals

**Goals:**

- построить тестируемый application core для permission decisions и relationship provisioning, независимый от tonic/sqlx;
- реализовать production inbound gRPC и outbound SpiceDB/PostgreSQL adapters с deny-by-default semantics;
- сохранить wire compatibility `gavel.authz.v1` и существующего Auction client;
- гарантировать durable idempotency relationship commands без distributed transaction со SpiceDB;
- сделать runtime, telemetry и Kubernetes configuration проверяемыми локально.

**Non-Goals:**

- проверять browser JWT или выполнять OAuth flow внутри Authz;
- хранить user profile, email либо доменные eligibility/state machines;
- добавлять permission allow-cache: deny-by-default важнее latency до benchmark;
- реализовывать BFF/Catalog/Payments/Notify consumers в этом change;
- управлять production SpiceDB/PostgreSQL cluster lifecycle из service chart.

## Decisions

### 1. Application core отделён от transport и storage

`contexts/authz/src/application/` содержит DTO/error taxonomy, use cases `CheckPermission`, `BulkCheckPermissions`, `RegisterPrincipal`, `MutateRelationships` и ports `PolicyEngine`, `CommandReceiptStore`, `WorkloadPolicy`. Он зависит только от устойчивых value types и async traits. `src/adapters/inbound/grpc.rs`, `src/adapters/outbound/spicedb.rs` и `postgres.rs` выполняют преобразования; `src/bootstrap.rs` только собирает runtime.

Authz не является владельцем Auction/Catalog/Payments rules: application layer проверяет schema-shaped references и caller scope, а фактический permission вычисляет SpiceDB. Альтернатива с checks в gRPC handlers отклонена: она смешивает security policy, retry/idempotency и transport mapping и делает contract tests зависимыми от сети.

### 2. Authz v1 расширяется только additive

В `contexts/authz/proto/gavel/authz/v1/authz_service.proto` добавляются:

- `RegisterPrincipal` с verified issuer/subject и idempotency key; response возвращает opaque principal id, written-at token либо typed rejection;
- поле `repeated Relationship deletions` в `WriteRelationshipsRequest`; существующее `updates` сохраняет номера и TOUCH semantics;
- новые response messages/enum values только с новыми field numbers, без изменения существующих полей.

Пустые `updates`/`deletions` запрещены; overlap одной tuple между TOUCH/delete отклоняется. Размер batch ограничивает config с безопасным default. Generated initializer в `contexts/auction/src/adapters/outbound/authz.rs` получает `deletions: Vec::new()`; это source-only адаптация, wire behavior Auction не меняется.

Альтернатива с отдельным `DeleteRelationships` RPC отклонена: единый batch должен атомарно выразить expand/contract relationship mutation и вернуть один ZedToken.

### 3. SpiceDB API vendored из зафиксированного upstream source

Минимальный набор официальных `authzed.api.v1` Proto и license/provenance фиксируется под `contexts/authz/proto/authzed/` на конкретном upstream revision и генерируется существующим Tonic 0.13 build. Обновление выполняется явной Make target с Buf-format/build verification, а production build не скачивает schemas из сети.

Это сохраняет единый Prost/Tonic ABI и Rust 1.85. Альтернативы отклонены: непроверенный `authzed` crate не даёт актуального полного API; crates на Tonic 0.14 потребовали бы workspace-wide upgrade; hand-written gRPC messages рискуют wire drift.

Outbound adapter использует один pooled channel, bearer credential из external secret, локальный timeout и официальный Permission/Watch-independent API. `CheckPermission` передаёт `at_least_as_fresh` при непустом token, иначе использует dependency default consistency. Bulk adapter использует официальный bulk permission RPC, сохраняет input order и требует один causal revision для ответа.

### 4. Workload identity берётся только из trusted proxy context

Inbound interceptor преобразует SPIFFE URI из sanitized XFCC/trusted request extension в закрытый enum workload (`bff`, `auction`, `catalog`, `payments`, `notify`, `processes`). Runtime принимает proxy-derived metadata только от loopback/явно настроенных trusted proxy CIDR; произвольные `x-principal-*` headers игнорируются. Dev/test identity injector включается отдельным fail-closed флагом, по умолчанию выключенным и запрещённым Helm production values.

Allowlist компилируется как закрытая матрица в Authz-owned application policy:

| Workload | Разрешённые mutations |
|---|---|
| `bff` | `RegisterPrincipal`; `operation:owner` |
| `catalog` | `lot:platform`, `lot:seller` |
| `auction` | `auction:platform`, `auction:lot`, `auction:participant` |
| `payments` | `payment_profile:owner`; `payment_attempt:platform/auction/owner` |
| `notify` | `notification:platform/recipient` |
| `processes` | relationship writes отсутствуют |

Read checks доступны только известным internal workloads. Validation всей mutation происходит до SpiceDB call. NetworkPolicy/Istio остаются первым барьером, но не заменяют application allowlist.

### 5. Principal id детерминирован и не хранит исходные claims

`RegisterPrincipal` нормализует issuer как валидный абсолютный HTTPS URI, запрещает пустой subject и вычисляет `u1_<base64url(HMAC-SHA256(key, length-prefix(issuer, subject)))>`. HMAC key поступает из external secret; issuer/subject используются только в памяти и не записываются в receipt payload/result. Результат содержит opaque id и token membership write.

Версия `u1_` оставляет путь к controlled rotation/migration. Простое hash без ключа отклонено из-за offline correlation известных `(iss, sub)`; случайный UUID потребовал бы хранить identity mapping и отдельный PII lifecycle.

### 6. Durable receipt claim предшествует внешнему эффекту

Forward-only migration `contexts/authz/migrations/` создаёт `authz_command_receipts` с ключом `(caller_scope, idempotency_key)`, 32-byte HMAC fingerprint, operation, status `PENDING|COMPLETED`, lease, attempts и безопасным JSON result. В таблице нет issuer, subject, relationship ids, credentials или ZedToken в открытых индексах; result хранит только opaque principal (если есть) и authoritative token.

Алгоритм mutation:

1. canonicalize/validate input и caller scope;
2. atomically claim отсутствующий key как `PENDING`; тот же fingerprint получает pending/replay state, другой — conflict;
3. после lease expiry только тот же fingerprint может увеличить attempt и продолжить;
4. выполнить один idempotent SpiceDB TOUCH/delete batch;
5. сохранить `COMPLETED` result; exact replay возвращает его до нового SpiceDB call.

Если процесс падает между шагами 4 и 5, повтор TOUCH/delete семантически безопасен и затем завершает receipt. Это намеренная saga без distributed transaction. Claim-before-effect предотвращает применение другого payload в ambiguity window. PENDING до expiry возвращает retryable contention, а не запускает параллельный write.

### 7. Error mapping закрыто и стабильно

Application errors: invalid input, unauthenticated workload, forbidden scope, idempotency conflict, command in progress, dependency unavailable и internal. gRPC mapping:

- invalid input → `INVALID_ARGUMENT`, oversized batch → `RESOURCE_EXHAUSTED`;
- missing/untrusted workload → `UNAUTHENTICATED`;
- caller scope violation → typed `DomainRejection` для write или `PERMISSION_DENIED` для RPC-level caller denial;
- idempotency mismatch → typed rejection `idempotency_conflict`;
- receipt/SpiceDB timeout/failure and in-progress lease → retryable `UNAVAILABLE`/`ABORTED`;
- expected authorization result остаётся `ALLOWED|DENIED`, не transport error.

Неизвестные dependency enum/status fail closed. Messages безопасны и не включают input values.

### 8. Runtime и readiness отражают реальные зависимости

Tokio runtime слушает отдельные HTTP и gRPC addresses. HTTP предоставляет `/healthz`, `/readyz`, `/metrics`; readiness становится true только после PostgreSQL connectivity/migrations-present check и SpiceDB readiness probe. Runtime запускает receipt recovery/cleanup worker, отслеживает его failure и graceful завершает listeners/workers по SIGTERM/SIGINT.

Schema и SQL migrations не применяются основным runtime. Binary subcommands выполняют только Authz receipts migration; отдельный Helm schema job применяет `schema/gavel.zed` к SpiceDB с external secret. Docker build использует monorepo root, workspace lock и Rust 1.85-compatible toolchain вместо текущего изолированного context build.

### 9. Telemetry имеет только bounded dimensions

Structured spans содержат operation, known workload, bounded outcome и trace/correlation context. Metrics покрывают single/bulk latency, allow/deny, invalid requests, SpiceDB failures, receipt claim/replay/conflict/recovery, relationship TOUCH/delete и worker health. Resource/principal/relationship ids, issuer/subject, idempotency key, tokens и credentials запрещены автоматическими tests.

Authz-owned Prometheus rules/dashboard заменяют bootstrap panels на decision failure ratio, dependency latency, deny/allow rate (не как security proof), receipt backlog/recovery и write failures. Cardinality contracts проверяются test/observability target.

### 10. Integration suite использует настоящие boundaries

`contexts/authz/Makefile` получает pinned disposable PostgreSQL и SpiceDB targets. Integration test загружает production schema, запускает real gRPC service/adapters и проверяет allow/deny, causal token, bulk order, caller matrix, registration replay/conflict, TOUCH/delete, crash-window recovery и отсутствие raw claims в receipts/logs. Contract tests запускают Buf/schema assertions и доказывают совместимость Auction client.

## Risks / Trade-offs

- **[Два datastore не дают atomic commit]** → claim-before-effect + idempotent TOUCH/delete + lease recovery исключают несовместимый второй payload и безопасно повторяют ambiguity window.
- **[Sanitized proxy identity настроена неверно]** → default deny, trusted-peer check, explicit Helm/Istio configuration и integration tests на spoofed metadata; production запрещает insecure injector.
- **[Vendored upstream Proto устаревает]** → provenance/revision фиксированы, обновление отдельно проходит Buf/build/integration; это предпочтительнее runtime network generation.
- **[HMAC key rotation меняет principal id]** → version prefix и отсутствие автоматической rotation; миграция key/relationships будет отдельным expand-migrate-contract change.
- **[Bulk request создаёт нагрузочный всплеск]** → hard item/byte limits, local deadline и bounded dependency message size.
- **[Receipt rows растут]** → retention применяется только к terminal receipts после периода retry; PENDING никогда не удаляется до recovery/ручного quarantine.

## Migration Plan

1. Добавить additive Proto и обновить Auction generated initializer; пройти Buf/build checks до server rollout.
2. Развернуть Authz receipts migration и external secret references; это не затрагивает SpiceDB graph.
3. Отдельным schema job подтвердить текущую `gavel.zed`; новых permission names change не требует.
4. Развернуть Authz runtime с relationship writes выключенными readiness/config flag, проверить permission checks и telemetry.
5. Включить registration/writes для BFF и владельцев по workload allowlist; consumers подключать последующими vertical slices.
6. Rollback binary безопасен, пока callers не используют новые RPC/deletions. Receipt schema forward-only остаётся; relationships откатываются только явными compensating deletes, не SQL rollback.
