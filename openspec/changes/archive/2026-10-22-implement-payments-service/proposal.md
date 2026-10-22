# Proposal

## Why

Payments сейчас остаётся hello-world scaffold: документированные правила платёжной готовности, provider-neutral списания и восстановления не исполняются, поэтому settlement нельзя безопасно довести от выбранного Auction кандидата до подтверждённого результата. Следующий vertical slice нужен сейчас, чтобы дать Processes authoritative payment owner RPC и завершить второй обязательный доменный контекст перед реализацией Temporal orchestration.

## What Changes

- Реализовать Payments-owned lifecycle платёжного инструмента и явного `DebitAuthorization`: начало provider-hosted регистрации, готовность, отзыв и безопасное публичное чтение.
- Реализовать provider-neutral `PaymentAttempt` с одной попыткой на `(auction_id, settlement_round)`, не более чем одним успешным платежом на аукцион и durable idempotency команд.
- Реализовать подписанные callbacks внешнего провайдера с дедупликацией, строгим отображением provider-specific состояний в нейтральные исходы и атомарным outbox.
- Сделать GraphQL Federation operations и Payments-owned gRPC `AttemptPayment`/`ExpirePayment` исполняемыми, авторизованными и наблюдаемыми; сохранить совместимость существующих Protobuf field numbers и SDL.
- Публиковать owner Protobuf module `buf.build/gavel/payments` в Buf Schema Registry и перевести Payments/Processes на точно закреплённые Generated Cargo SDK packages вместо локальной компиляции чужих `.proto`.
- Добавить PostgreSQL storage, Authz integration, первый Stripe adapter, runtime bootstrap, deployment/observability assets и contract/integration tests.
- Подтвердить существующий асинхронный контракт `PaymentInstrumentReady`/`PaymentInstrumentRevoked` для Auction и добавить versioned payment-attempt events для Processes/Analytics без provider-specific данных.

## Capabilities

### New Capabilities

- `payment-instrument-readiness`: Регистрация, явное разрешение на списание, готовность и отзыв платёжного инструмента без хранения карточных данных.
- `payment-attempt-lifecycle`: Provider-neutral создание, продолжение, успех, восстановление и истечение платёжной попытки с authoritative инвариантами Payments.
- `payment-provider-callbacks`: Проверка подписи, дедупликация и безопасное применение внешних callback к инструментам и попыткам.

### Modified Capabilities

- `internal-rpc-contracts`: Зафиксировать Payments-owned `AttemptPayment`/`ExpirePayment` как доверенные, bounded-deadline и идемпотентные команды settlement saga.

## Impact

- **Основной владелец:** bounded context Payments; основные изменения находятся в `contexts/payments/`, включая domain/application modules, PostgreSQL migrations, GraphQL, Protobuf, Stripe adapter, callback ingress и operations.
- **Authz:** `contexts/authz/` остаётся владельцем permission decisions и relationships; Payments использует существующий Authz RPC и при необходимости additive расширяет SpiceDB schema для `payment_profile`/`payment_attempt` relations и permissions.
- **Auction:** остаётся владельцем eligibility и settlement calculation; его consumer продолжает принимать versioned `PaymentInstrumentReady`/`PaymentInstrumentRevoked` и проверяется contract/E2E tests без синхронного вызова Payments.
- **Processes:** остаётся владельцем saga, recovery timer и перехода к следующему кандидату; в этом change он потребляет закреплённый Payments Generated SDK из BSR и проверяет owner RPC contract, но Temporal workflow не реализуется.
- **BFF/edge:** Cosmo Router только компонует Payments SDL; provider callback идёт отдельным Cloudflare/Istio route и авторизуется подписью провайдера, а не browser JWT.
- **Contracts and compatibility:** исходные `.proto` остаются в `contexts/payments/proto` под владением Payments, а BSR `buf.build/gavel/payments` становится versioned distribution seam. Изменения публикуются сначала под change label, проходят registry/local lint и breaking checks, затем consumers закрепляют точные версии Generated Prost/Tonic Cargo SDK; Protobuf и GraphQL изменяются только additive.
- **Build/CI:** `.cargo/config.toml` подключает BSR Cargo registry, credentials передаются только через developer/CI secret store. Payments и новые consumers не используют локальный `protoc`, `tonic-build`, `include_proto!` или checked-in generated stubs; `Cargo.lock` фиксирует разрешённые SDK packages.
- **Data/rollout:** добавляется owned `payments-db` schema с transactional outbox, inbox/callback deduplication и command receipts; миграция создаёт новые таблицы без переноса существующих production данных.
- **Вне scope:** refunds, seller payouts, penalties, повторный допуск forfeited кандидата, выбор дополнительного provider и реализация Temporal workflow.
