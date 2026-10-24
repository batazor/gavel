# Payments

Платёжная готовность участника и выполнение платежа через сменных провайдеров. Контекст хранит бизнес-состояние платёжного инструмента, согласие на автоматическое списание и попытки оплаты, но **не данные карты и не процесс выбора победителя**.

Stripe — первый внешний адаптер. Детерминированный sandbox — отдельный local-only адаптер того же `ProviderGateway`. Домен Payments не использует их объекты и статусы: выбор адаптера не меняет команды, события или правила Auction и Processes.

## ADR

- [0001 — Инициализация проекта](adr/0001-init.md)

## Interface

- [Federation subgraph SDL](graphql/schema.graphql) — пользовательские registration/recovery/status operations; provider callbacks остаются отдельным ingress.
- [gRPC contract](proto/gavel/payments/v1/payments_service.proto) — provider-neutral команды и authoritative `GetSettlementAttempt` для Processes; правила — в [init ADR](adr/0001-init.md).
- [BSR SDK provenance](bsr-sdk.lock.json) — immutable module commit и exact Prost/Tonic package versions, используемые Payments и Processes.
- [Глоссарий](GLOSSARY.md) — канонические термины bounded context.

## Язык

| Термин | Значение |
|---|---|
| Платёжный инструмент | Непрозрачная ссылка на сохранённый у провайдера способ оплаты |
| Разрешение на списание | Явное согласие участника на автоматическую оплату, если Auction выбрал его settlement candidate |
| Готов к торгам | Инструмент подготовлен провайдером, а действующее разрешение зафиксировано; это не гарантия наличия денег |
| Платёжная попытка | Провайдер-независимая попытка списать заданную сумму для `(auction_id, settlement_round)` |
| Требуется восстановление | Автоматическое списание не завершилось, участнику нужно подтвердить оплату или заменить инструмент |
| Callback провайдера | Подписанное внешнее уведомление, преобразуемое адаптером в нейтральный исход Payments |

## Агрегаты

### `PaymentInstrument`

Состояние: `Missing → Ready → Revoked`. Хранит тип провайдера, непрозрачные `customer_ref` и `instrument_ref`, recovery email, безопасные display-поля и `DebitAuthorization(terms_version, accepted_at)`.

**Команды:** `BeginInstrumentRegistration`, `MarkInstrumentReady`, `RevokeInstrument`.

**События:** `PaymentInstrumentReady`, `PaymentInstrumentRevoked` → `payments.events`.

### `PaymentAttempt`

Состояние: `Pending → Processing | RecoveryRequired | Succeeded | Expired`. Адаптер переводит provider-specific ответы в нейтральные результаты; домен не знает их названий.

**Команды:** `AttemptPayment(auction_id, round, candidate, amount)`, `ContinuePayment`, `ExpirePayment`.

**Process-only read:** `GetSettlementAttempt(attempt_id, expected_auction_id, expected_round)` доступен только trusted Processes SPIFFE workload и повторно проверяет permission через Authz. Binding с auction/round fail-closed. Recovery URL возвращается только для `RecoveryRequired`, payment id — только для `Succeeded`; provider refs не покидают Payments.

**События:** `PaymentAttemptStarted`, `PaymentRecoveryRequired`, `PaymentSucceeded`, `PaymentExpired` → restricted topic `gavel.payments.attempts.v1` с partition key `aggregate_id`.

## Инварианты

- К ставке допускается только участник с `PaymentInstrument.Ready` и действующим `DebitAuthorization`.
- Готовый инструмент не резервирует средства и не гарантирует успешное будущее списание.
- Не более одного `PaymentSucceeded` на `auction_id`.
- Одна платёжная попытка на `(auction_id, settlement_round)`; тот же ключ передаётся адаптеру для идемпотентности.
- Payments получает кандидата и сумму от Auction через Processes; он не ранжирует участников и не рассчитывает цену.
- Callback обрабатывается идемпотентно по непрозрачному `provider_event_ref`.
- Provider-specific объекты и статусы остаются внутри адаптера.
- Recovery URL не публикуется в `payments.events`, GraphQL process API, workflow history, метрики или обычные логи; Processes получает его кратковременно только через authoritative RPC и передаёт Notify.

## Интеграции

- BFF начинает регистрацию инструмента и открывает выданный провайдером URL.
- Auction поддерживает проекцию `PaymentInstrumentReady`/`PaymentInstrumentRevoked` для допуска к ставкам.
- Processes вызывает платёжные команды и `GetSettlementAttempt`, а также владеет уведомлениями, двухчасовым recovery timer и переходом к следующему кандидату. Все RPC требуют trusted SPIFFE identity, bounded deadline и permission `platform:gavel#operate_payments@service_account:processes`.
- Payment provider port поддерживает `PrepareInstrument`, `Charge`, `ContinuePayment`, `Cancel`; production adapter реализован через Stripe, local adapter — через deterministic sandbox.
- Внешние callbacks проходят через Cloudflare и отдельный Istio route без JWT/browser challenge/cache; подпись всё равно проверяет конкретный адаптер.
- Transactional outbox атомарно сохраняет attempt envelope для `gavel.payments.attempts.v1`; Processes — единственный разрешённый reader MVP topic. Provider refs, provider objects и recovery URL в envelope отсутствуют.

## Юзкейсы

- [register-payment-instrument](use-cases/register-payment-instrument/README.md) — подготовить платёжный инструмент до участия в торгах
- [settle-payment-attempt](use-cases/settle-payment-attempt/README.md) — выполнить success, recovery либо expiry через одну provider-neutral границу
- [settle-auction](../processes/use-cases/settle-auction/README.md) — Temporal-процесс автоматической оплаты и перехода к следующему кандидату

## Позже

Дополнительные payment provider adapters, возвраты и выплаты продавцу.

## Operations

- [Production bootstrap](src/main.rs) — GraphQL, signed callback, health/readiness/metrics и gRPC listeners с reconciliation worker.
- Local profile включается только сочетанием `PAYMENTS_RUNTIME_PROFILE=local` и `PAYMENT_PROVIDER_ADAPTER=sandbox`. `PAYMENT_SANDBOX_DEFAULT_OUTCOME` принимает `success`, `recovery_then_success` либо `expire`; route управления outcome и подтверждения sandbox-инструмента требует bearer token `PAYMENT_SANDBOX_CONTROL_TOKEN`. При любом другом runtime profile процесс отказывается запускать sandbox.
- Processes RPC и GraphQL/health/metrics работают в том же supervised runtime независимо от выбранного provider adapter. Stripe callback route монтируется только для Stripe, sandbox control route — только для local sandbox.
- [Docker image, Helm chart, Prometheus rules и Grafana dashboard](ops/Helm/README.md)
