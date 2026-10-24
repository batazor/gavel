# Tasks

## 1. Auction restricted process events

- [x] 1.1 Добавить owner contract `contexts/auction/proto/gavel/auction/v1/auction_process_events.proto` с versioned envelope и `SettlementCandidateSelected`/terminal payloads, не меняя privacy-safe `AuctionPublicEvent`; проверить `make buf-check` и negative contract test на отсутствие candidate/maxima в public descriptor.
- [x] 1.2 Добавить additive migration `auction_process_outbox` с ограничениями и aggregate ordering index; проверить повторяемость migration и schema constraints через `make -C contexts/auction database-check`.
- [x] 1.3 Реализовать Auction process-event mapping/codec для selected candidate и terminal facts с correlation/causation, version и безопасной сериализацией; проверить round-trip, unknown-version и privacy tests.
- [x] 1.4 Расширить Auction unit of work так, чтобы private events, projection, public outbox и restricted process outbox фиксировались атомарно; проверить rollback, replay и concurrent forfeiture integration tests без двойного process event.
- [x] 1.5 Обновить Auction use-case README/Mermaid и `close-by-deadline/flow.c4`, явно разделив public и restricted event paths; проверить `make arch-validate` и относительные ссылки.

## 2. Payments authoritative process read

- [x] 2.1 Additive расширить `gavel.payments.v1.PaymentsService` RPC `GetSettlementAttempt` с expected auction/round binding и provider-neutral response; проверить Buf format/lint/build/breaking и отсутствие recovery URL в payment event schema.
- [x] 2.2 Опубликовать preview Payments module, обновить exact BSR Generated Prost/Tonic SDK versions у Payments и Processes и сохранить commit provenance; проверить clean build без локальной компиляции Payments proto.
- [x] 2.3 Реализовать Payments application use case для process-only чтения attempt с fail-closed workload/Authz check, auction/round binding и state-dependent recovery data; проверить allow/deny/mismatch/stale/not-found/privacy unit tests.
- [x] 2.4 Реализовать tonic adapter и error mapping `GetSettlementAttempt`, принимающий только trusted Processes SPIFFE identity и bounded deadline; проверить spoofing, timeout, safe typed outcomes и in-process gRPC tests.
- [x] 2.5 Обновить Payments/Processes contract docs и authorization assertions без расширения публичного GraphQL SDL; проверить `make authz-validate`, `make graphql-check` и schema privacy tests.

## 3. Processes Temporal foundation

- [x] 3.1 Exact-pin Temporal Rust SDK/client/common/macros 1.1.0 в workspace, поднять MSRV только `gavel-processes` до 1.92 и обновить lockfile; проверить `cargo check -p gavel-processes --all-targets` и неизменный MSRV остальных crates.
- [x] 3.2 Создать module boundaries `workflows`, `activities`, `ingress`, `adapters`, `bootstrap`, `telemetry` и typed process DTO/errors; проверить compile-time fakes и отсутствие framework types в workflow DTO.
- [x] 3.3 Добавить architecture tests, запрещающие tonic/HTTP/I/O/system time/random/Tokio concurrency в workflow modules и локальные копии owner contracts; проверить `cargo test -p gavel-processes --test architecture_boundaries`.
- [x] 3.4 Расширить `rpc_policy` typed retry/activity options и генерацию всех stable intent/notification keys; проверить exhaustive code classification, round separation и run-id independence unit tests.

## 4. Version-aware signal ingress

- [x] 4.1 Реализовать typed decoders/validation для Auction public lifecycle events, Auction restricted process events и Payments attempt events; проверить supported versions, malformed payload, unknown type и secret-field rejection fixture tests.
- [x] 4.2 Реализовать fingerprint dedupe, monotonic aggregate/attempt versions и quarantine для changed redelivery; проверить duplicate/stale/out-of-order/gap scenarios детерминированными unit tests.
- [x] 4.3 Реализовать Temporal gateway с `signal-with-start`, стабильными workflow/request IDs и `USE_EXISTING` conflict policy; проверить, что concurrent duplicate events создают один workflow и один логический signal.
- [x] 4.4 Добавить broker-agnostic ingress port и test adapter без production HTTP/manual backdoor; проверить contract test, который подаёт fixture и наблюдает workflow state через Temporal client.

## 5. Owner RPC activities

- [x] 5.1 Реализовать Auction activities `CloseAuction`, `ForfeitSettlementCandidate`, `ConfirmWinner` поверх owner-generated client с trusted metadata, 3s deadline и typed outcomes; проверить replayed request, domain rejection и transient transport tests.
- [x] 5.2 Реализовать Payments activities `AttemptPayment`, `GetSettlementAttempt`, `ExpirePayment` с immutable round binding и redacted results; проверить processing/recovery/success/expiry и mismatch tests.
- [x] 5.3 Реализовать Notify activities с deterministic notification IDs; recovery activity должна читать URL у Payments и передавать Notify, возвращая workflow только redacted acknowledgement; проверить history/log/metric fixtures на отсутствие URL.
- [x] 5.4 Настроить Temporal activity retry policy: максимум 4 attempts, exponential backoff и retry только transient failures; проверить response-lost-after-commit и non-retryable rejection на local Temporal test environment.

## 6. AuctionLifecycle workflow

- [x] 6.1 Реализовать сериализуемое state и version-aware handlers для `AuctionOpened`, `DeadlineExtended`, `AuctionRescheduled` и terminal signals; проверить duplicate/stale/out-of-order transition table tests.
- [x] 6.2 Реализовать deterministic durable timer loop и логическую замену timer при новом дедлайне; проверить time-skipping test, что старый deadline не вызывает `CloseAuction`.
- [x] 6.3 Реализовать `CloseAuction` ветки success, `too_early`, already-terminal и retryable failure без локального доменного решения; проверить stable idempotency и ровно один owner effect после worker restart.
- [x] 6.4 Добавить safe workflow query/search attributes для close deadline/confirmation без auction/user IDs в metric labels; проверить query shape, overdue watchdog input и replay determinism.
- [x] 6.5 Сохранить representative lifecycle histories для open/reschedule/too-early/restart и добавить replay gate; проверить replay текущим worker без nondeterminism.

## 7. SettleAuction workflow

- [x] 7.1 Реализовать round state machine и version-aware signals для candidate/payment/terminal owner events; проверить stale round, changed duplicate и monotonic transition table tests.
- [x] 7.2 Реализовать `AttemptPayment` и ожидание authoritative outcomes, не завершая workflow в `PROCESSING`; проверить synchronous success/recovery и asynchronous callback paths.
- [x] 7.3 Реализовать двухчасовой durable recovery timer и deterministic race с Payments signal; проверить time-skipping success-before-expiry, expiry-before-late-success и ambiguous-cancel scenarios.
- [x] 7.4 Реализовать `ExpirePayment → ForfeitSettlementCandidate → next restricted round` и terminal `UNSOLD`; проверить, что новый amount приходит только от Auction и closing price не вычисляется Processes.
- [x] 7.5 Реализовать `PaymentSucceeded → ConfirmWinner → SOLD` с candidate/payment binding и stale-success protection; проверить не более одного confirm effect при duplicate event и worker restart.
- [x] 7.6 Интегрировать settlement/recovery/winner notifications как независимые effects, не сдвигающие timer и не меняющие domain outcome; проверить transient и permanent Notify failures.
- [x] 7.7 Сохранить representative settlement histories для success, recovery, expiry/forfeiture, next round и restart и добавить replay gate; проверить отсутствие recovery URL/provider refs/maxima в history payloads.

## 8. Processes runtime, operations and documentation

- [x] 8.1 Заменить hello-world `main.rs` на Tokio bootstrap с config validation, Temporal client/worker registration, HTTP health/readiness/metrics и graceful drain; проверить startup failure, poller readiness, Temporal disconnect и shutdown tests.
- [x] 8.2 Реализовать SDK/custom metrics, safe structured logs/traces и completion observer один раз на terminal transition; проверить bounded labels и отсутствие IDs, amounts, keys и recovery URL.
- [x] 8.3 Обновить Processes Dockerfile и Helm chart: Temporal endpoint/namespace/queues, secretKeyRefs, production replicas, PDB/topology, probes и resources; проверить clean image build и `make -C contexts/processes helm-check` render assertions.
- [x] 8.4 Добавить pinned local Temporal dev-server lifecycle и `integration-check` Make targets; проверить synthetic start → timer → activity → completion и корректный cleanup.
- [x] 8.5 Синхронизировать Processes README, settlement README/Mermaid, close-by-deadline docs, observability runbook и LikeC4 flows с фактическими task queues/signals/activities; проверить `make arch-validate` и documented commands.
- [x] 8.6 Обновить Prometheus rules/Grafana dashboard для worker readiness, schedule-to-start, activity failures, signal gaps, unexpected terminal outcomes и overdue close; проверить `make -C contexts/processes observability-check`.

## 9. Cross-context release verification

- [x] 9.1 Прогнать local Temporal E2E с реальными Auction/Payments/Notify in-process gRPC boundaries: open → reschedule → close → pay success → confirm; проверить один process workflow и один effect каждого владельца.
- [x] 9.2 Прогнать E2E recovery flow: callback recovery → notification → двухчасовой timer → expire → forfeiture → next round → success/unsold, включая restart/replay и race; проверить terminal convergence.
- [x] 9.3 Проверить owner persistence на disposable PostgreSQL командами `make -C contexts/auction database-check` и `make -C contexts/payments database-check`; подтвердить atomic outboxes, receipts и process read.
- [x] 9.4 Выполнить compatibility/privacy gates `make buf-check`, registry-based Buf breaking, `make graphql-check`, `make authz-validate` и negative schema/history tests; подтвердить отсутствие breaking public changes и утечек restricted данных.
- [x] 9.5 Выполнить `make -C contexts/processes check integration-check helm-check observability-check`, `make -C contexts/auction check`, `make -C contexts/payments check`, `make arch-validate` и `make openspec-validate`; все доступные проверки должны быть зелёными, недоступные внешние gates явно зафиксированы.

## Workflow follow-up

- После реализации и review синхронизировать capability spec и архивировать `implement-processes-temporal-workflows`.
- После зелёных consumer/package gates продвинуть проверенный Payments BSR preview commit на release/default label и сохранить rollback version.
- Production event ingress включать только последующим Redpanda/Debezium change после topic ACL, connector, offset/replay и capacity checks.
