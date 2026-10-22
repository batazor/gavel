# Подготовить платёжный инструмент до участия в торгах

Контекст: Payments · Актор: участник · Диаграмма: `flow.c4` → `flow_register_payment_instrument`

## Цель

До первой ставки подготовить у выбранного платёжного провайдера повторно используемый инструмент и зафиксировать разрешение участника на автоматическое списание.

## Условия разрешения

Пользователь явно соглашается, что:

- после выбора settlement candidate Gavel попытается списать рассчитанную Auction сумму без присутствия пользователя;
- сумма определяется правилами аукциона и не превышает его принятого максимума;
- если автоматическая оплата не завершилась, Temporal-процесс даёт два часа на восстановление;
- после истечения окна кандидат выбывает, и предложение переходит следующему участнику.

Payments хранит `terms_version` и `accepted_at`. Отзыв разрешения запрещает новые ставки, но не отменяет обязательства по уже принятым ставкам.

## Основной сценарий

```mermaid
sequenceDiagram
    autonumber
    actor Bidder as Участник
    participant Web
    participant Router as Cosmo Router
    participant Payments
    participant Provider as Payment provider
    participant Authz
    participant DB as payments-db
    participant Broker as Redpanda
    participant Auction

    Bidder->>Web: Добавить платёжный инструмент<br/>и принять terms_version
    Web->>Router: beginPaymentInstrumentRegistration
    Router->>Payments: BeginInstrumentRegistration<br/>(user, email, terms_version)
    Payments->>Payments: Fail-closed register permission
    Payments->>Provider: PrepareInstrument<br/>stable opaque idempotency key
    Payments->>Authz: TOUCH owner grant
    Provider-->>Web: Provider-hosted registration
    Bidder->>Provider: Enter payment details
    Note over Bidder,Provider: Payment details never enter Gavel
    Provider-->>Payments: Signed raw-body callback
    Payments->>Payments: Verify signature, claim inbox lease<br/>and map to InstrumentPrepared
    Payments->>DB: Save opaque refs + DebitAuthorization<br/>+ PaymentInstrumentReady outbox
    Payments-->>Router: Registration status
    Router-->>Web: Instrument READY
    Broker-->>Auction: PaymentInstrumentReady(user_id)
    Auction->>Auction: Update local eligibility projection
```

## Ошибки и изменения

- Провайдер не подготовил инструмент → он не становится `Ready`, ставка запрещена.
- Ошибка Authz закрывает операцию; grant создаётся до локального commit, а при его отказе выполняется compensating delete. Worker повторно удаляет grant отозванных инструментов.
- Инструмент удалён или разрешение отозвано → `PaymentInstrumentRevoked`; новые ставки запрещены.
- Формально готовый инструмент может не сработать позже; фактическая попытка выполняется только после выбора settlement candidate.
- Callback подтверждается только после durable state/outbox commit. Незавершённый inbox claim возвращает retryable status и после lease может быть безопасно захвачен повторно.
- Повторы регистрации используют один HMAC-derived provider key; неопределённый ответ провайдера или потерянный локальный ответ не создают второй provider session.

## Важное ограничение

Готовность означает возможность попытки будущего списания. Она не резервирует сумму и не гарантирует успешную оплату.
