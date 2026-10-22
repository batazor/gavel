# Tasks

## 1. Payments contracts and crate foundation

- [x] 1.1 Зарегистрировать в root `buf.yaml` все уже созданные owner modules `buf.build/gavel/{auction,authz,catalog,payments,notify}` с их context-owned proto paths; проверить `buf format --diff`, `buf lint`, `buf build` и отсутствие неименованных modules.
- [x] 1.2 Additive расширить `gavel.payments.v1` correlation/causal/typed outcome полями без изменения существующих field numbers и проверить breaking compatibility непосредственно против default-label `buf.build/gavel/payments`, а также `make buf-check`.
- [x] 1.3 Опубликовать проверенную Payments schema в BSR под label `implement-payments-service`, получить новый module commit и через `buf registry sdk info/version` разрешить package names и exact Generated Prost/Tonic SDK versions; сохранить проверяемую commit/version provenance в contract test fixture или machine-readable config.
- [x] 1.4 Добавить `.cargo/config.toml` для BSR sparse Cargo registry и CI/developer authentication через external token provider без credentials в repository; проверить authenticated registry access и secret-scanning assertions.
- [x] 1.5 Перевести Payments crate с `build.rs`/`protoc-bin-vendored`/`tonic-build`/`include_proto!` на exact BSR Generated SDK dependencies и context-owned Rust re-export; проверить чистую сборку в новом target directory без `protoc` и без `target/buf`.
- [x] 1.6 Перевести Processes payment client на тот же exact Payments Generated SDK commit, запретив copied `.proto`/generated stubs и floating versions; проверить compile-time client/server compatibility и `Cargo.lock` registry source/version.
- [x] 1.7 Определить versioned `PaymentInstrumentReady`, `PaymentInstrumentRevoked`, `PaymentAttemptStarted`, `PaymentRecoveryRequired`, `PaymentSucceeded`, `PaymentExpired` envelopes/fixtures без provider refs и recovery URL; проверить contract tests и существующий `contexts/auction/tests/event_consumers.rs`.
- [x] 1.8 Расширить Payments Federation SDL только delivery-safe полями/errors, сохранить существующие operations и проверить `make graphql-check` плюс schema privacy tests.
- [x] 1.9 Заменить scaffold crate dependencies/features на workspace-compatible Tokio, async-graphql, SQLx, tonic, HTTP/TLS/crypto/telemetry stack и проверить `cargo metadata --no-deps` и `cargo check -p gavel-payments --all-targets`.
- [x] 1.10 Добавить architecture boundary tests, запрещающие domain/application imports из SQLx, Stripe, tonic и GraphQL adapters, а RPC consumers — локальную компиляцию owner `.proto`; проверить `cargo test -p gavel-payments --test architecture_boundaries`.
- [x] 1.11 Обновить ADR-0018 и Payments/Processes contract documentation для BSR module ownership, preview/release labels, exact Generated SDK pinning и rollback; проверить относительные ссылки и отсутствие registry credentials/package-name догадок в документации.

## 2. Payments domain model

- [x] 2.1 Реализовать typed ids, validated email/terms/provider refs, `Money`/currency и redacted display values с unit/property tests на invalid/overflow/privacy cases.
- [x] 2.2 Реализовать `DebitAuthorization` и `PaymentInstrument` transitions `PENDING → READY → REVOKED`, включая server time/version и запрет повторного/недопустимого перехода; проверить exhaustive domain tests.
- [x] 2.3 Реализовать `PaymentAttempt` transitions `PENDING → PROCESSING | RECOVERY_REQUIRED | SUCCEEDED | EXPIRED`, immutable auction/round/candidate/amount и terminal guards; проверить transition table tests.
- [x] 2.4 Зафиксировать one-attempt-per-round и one-success-per-auction как domain commit intents/typed conflicts, не помещая settlement ranking в Payments; проверить concurrent model tests с двумя rounds.
- [x] 2.5 Реализовать provider-neutral outcomes/events и safe serialization без provider-specific vocabulary/secrets; проверить snapshot/negative privacy tests.
- [x] 2.6 Обновить `contexts/payments/GLOSSARY.md` и README агрегатов только каноническими терминами, проверить ссылки и отсутствие implementation details в glossary.

## 3. Application seams and durable command execution

- [x] 3.1 Определить узкие ports для unit of work, provider gateway, Authz, clock/id generation и telemetry и application DTO/error taxonomy; проверить compile-time fakes и stable safe error codes.
- [x] 3.2 Реализовать общий HMAC canonical command receipt/executor с claim/replay/conflict/lease recovery; concurrent in-memory tests должны доказать one-effect semantics и отсутствие raw payload/key в persistence/telemetry.
- [x] 3.3 Реализовать `BeginInstrumentRegistration` с validation, fail-closed Authz, owner grant provision, стабильным provider key и replay; unit tests должны покрыть allow/deny/timeout/provider uncertainty/local commit loss.
- [x] 3.4 Реализовать callback-driven readiness и `RevokeInstrument` с atomic state/event result и conservative orphan-grant reconciliation; unit tests должны покрыть ready/revoked/stale/replay/rollback paths.
- [x] 3.5 Реализовать authorized `myPaymentInstrument` query с safe not-found/dependency errors; unit tests должны доказать owner isolation и отсутствие refs/revisions.
- [x] 3.6 Синхронизировать registration README Mermaid и `flow.c4` с consent, grants, callback replay и failure paths; проверить `make arch-validate` и относительные Payments links.

## 4. Payment attempt application lifecycle

- [x] 4.1 Реализовать `AttemptPayment` с trusted Processes policy, immutable input, provider key `settlement:{auction}:{round}` и durable replay; unit tests должны покрыть processing/recovery/success/deny/timeout/conflict.
- [x] 4.2 Реализовать `ContinuePayment` с candidate ownership, `continue_payment` permission и reuse той же provider operation; unit tests должны покрыть URL/result privacy, terminal и чужую attempt.
- [x] 4.3 Реализовать `ExpirePayment` с provider query/cancel-before-expire и success-wins contract; deterministic race tests должны покрыть success-before-expiry, expiry-before-late-event и uncertain cancel retry.
- [x] 4.4 Реализовать application event/outbox mapping для всех attempt transitions без provider objects; contract tests должны доказать один event на commit и отсутствие второго event при replay.
- [x] 4.5 Добавить reconciliation use cases для stale `PROCESSING`, abandoned command claims и unknown callbacks без самостоятельного выбора business outcome; проверить bounded batch/idempotent retry tests.
- [x] 4.6 Обновить settlement README Mermaid/LikeC4 только в owned/shared paths для фактических RPC/event/race semantics; проверить `make arch-validate`.

## 5. Provider callback and Stripe adapter

- [x] 5.1 Реализовать raw-body callback interface и `VerifiedProviderEvent` mapping, не пропуская Stripe types в application layer; compile tests должны подтвердить seam.
- [x] 5.2 Реализовать Stripe signature/timestamp verification с constant-time comparison, secret redaction и fixture tests на valid/invalid/stale/malformed inputs.
- [x] 5.3 Реализовать Stripe `prepare`, `charge`, `continue`, `cancel/query` с deadlines, stable idempotency headers и нейтральным outcome mapping; contract tests против deterministic HTTP fake должны покрыть success/recovery/processing/decline/timeout/malformed response.
- [x] 5.4 Реализовать callback dedup по `(provider,event_ref)+HMAC fingerprint`, monotonic aggregate transition и provider-compatible acknowledgement; concurrent tests должны доказать exact replay, conflict и один outbox effect.
- [x] 5.5 Добавить HTTP callback handler на `/payment-provider-callbacks/stripe`, проверяющий signature до domain mapping и возвращающий retryable status до durable commit; in-process tests должны покрыть raw body preservation и отсутствие GraphQL/JWT dependency.
- [x] 5.6 Добавить security/privacy tests для errors, logs, metrics и serialized responses: raw body, signatures, refs, emails, amounts, recovery URL и credentials не должны утекать.

## 6. PostgreSQL and Authz integration

- [x] 6.1 Добавить migration для instruments/authorizations, attempts, receipts, callback inbox и outbox с FK/check/unique/partial-success indexes; проверить real PostgreSQL migration и schema constraint tests.
- [x] 6.2 Реализовать SQLx instrument unit of work с atomic profile/state/result/outbox и optimistic versioning; integration tests должны покрыть commit/rollback/replay/concurrent mutation.
- [x] 6.3 Реализовать SQLx attempt unit of work с row locking, one-round и one-success constraints; integration tests должны доказать один winner при concurrent callbacks/expiry и отсутствие partial outbox.
- [x] 6.4 Реализовать SQLx receipt/inbox repositories с lease recovery и fingerprint conflict; real PostgreSQL tests должны покрыть concurrent claim, duplicate callback и crash recovery.
- [x] 6.5 Реализовать safe read repositories и bounded reconciliation scans/indexes; integration tests должны покрыть owner query, pagination/batch limit и explain-friendly predicates.
- [x] 6.6 Additive расширить owner-scoped SpiceDB schema для `payment_profile`/`payment_attempt`, обновить Authz allowlist и проверить `make authz-validate` плюс unauthorized cross-owner tests.
- [x] 6.7 Реализовать Payments Authz gRPC adapter для register/manage/view/continue/process и atomic TOUCH/delete с deadlines/causal token; in-process tests должны покрыть allow/deny/order/token/fail-closed и orphan cleanup.
- [x] 6.8 Добавить `database-up/down/check` и `integration-check` targets с pinned disposable PostgreSQL; проверить `make -C contexts/payments database-check`.

## 7. GraphQL and gRPC inbound adapters

- [x] 7.1 Реализовать Federation queries/mutations для registration, own instrument, own attempt и continue через application interfaces; resolver tests должны покрыть success/replay/typed safe errors и context authentication.
- [x] 7.2 Реализовать tonic `AttemptPayment`/`ExpirePayment` с mandatory bounded deadline, validation и typed outcome/status mapping; in-process tests должны покрыть success/replay/domain/dependency/invalid paths.
- [x] 7.3 Реализовать trusted-proxy/workload identity extraction: HTTP principal только из router context, gRPC Processes только из authenticated mesh SPIFFE identity; spoofing tests должны отклонять payload/metadata подмену.
- [x] 7.4 Добавить Processes/Payments gRPC E2E через exact BSR Generated SDK package и Federation composition check, проверив module commit provenance, idempotency/correlation/deadline и отсутствие secret/high-cardinality data в errors.

## 8. Runtime, observability and deployment

- [x] 8.1 Заменить hello-world binary на Tokio bootstrap с HTTP GraphQL/callback/health/ready/metrics и gRPC listeners, dependency readiness, supervised workers и graceful shutdown; bootstrap tests должны покрыть routing/readiness/fatal-worker/shutdown.
- [x] 8.2 Собрать production composition root с PostgreSQL, Authz и Stripe adapters, config validation и secret redaction; startup tests должны fail fast на missing/invalid secrets без печати values.
- [x] 8.3 Реализовать bounded metrics/tracing для commands/provider/callback/outbox/reconciliation и backlog-age gauges; tests должны перечислить labels и запретить entity/payment identifiers.
- [x] 8.4 Обновить multi-stage monorepo Dockerfile на pinned Rust-compatible base/non-root runtime и проверить `make -C contexts/payments image`.
- [x] 8.5 Обновить Helm chart: 3 replicas, HTTP/gRPC services, migration Job, external secretKeyRefs, probes, resources, PDB/topology, callback route policy и trusted RPC authorization; `make -C contexts/payments helm-check` и render assertions должны пройти без Kubernetes Secret.
- [x] 8.6 Обновить Prometheus rules, Grafana dashboard и runbook для availability, provider failures, invalid signatures, reconciliation/outbox lag и restart storm; `make -C contexts/payments observability-check` должен пройти.

## 9. Cross-context and release verification

- [x] 9.1 Добавить real boundary flow registration → signed callback → readiness outbox → Auction duplicate-safe projection на disposable PostgreSQL/provider fake; проверить `make -C contexts/payments integration-check` и `make -C contexts/auction check`.
- [x] 9.2 Добавить real settlement flow AttemptPayment → recovery/success/expiry callbacks через gRPC и PostgreSQL с restart/replay/race cases; проверить один provider effect, один success на auction и typed uncertain outcome.
- [x] 9.3 Проверить public contract privacy и compatibility командами `make buf-check`, registry-based `make buf-breaking`, `make graphql-check` и fixture/schema negative tests; подтвердить, что exact SDK version разрешается к ожидаемому Payments BSR commit и что preview package не стал release dependency до promotion.
- [x] 9.4 Проверить архитектуру/документацию командами `make arch-validate`, относительным link check и сверкой README Mermaid с `flow.c4`.
- [x] 9.5 Выполнить release matrix `make -C contexts/payments check database-check integration-check helm-check observability-check`, `make -C contexts/auction check`, `make authz-validate`, BSR SDK provenance/clean-build checks и `make openspec-validate`; все доступные проверки должны быть зелёными.

## Workflow follow-up

- После implementation и release matrix синхронизировать delta-specs и архивировать `implement-payments-service`.
- После зелёных consumer/package gates продвинуть проверенный Payments module commit с change label на release/default label и сохранить exact SDK rollback version.
- После готовности Catalog и Payments owner RPC перейти к отдельному change для Processes/Temporal `SettleAuction` orchestration.
