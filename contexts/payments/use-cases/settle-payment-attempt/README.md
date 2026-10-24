# Выполнить платёжную попытку локального settlement

Контекст: Payments · Акторы: Processes, участник · Диаграмма: `flow.c4` → `flow_settle_payment_attempt`

## Цель

Выполнить одну authoritative попытку оплаты для `(auction_id, settlement_round)` через общий `ProviderGateway`, сохранив тот же доменный и outbox path для Stripe и local sandbox.

## Основной сценарий

```mermaid
sequenceDiagram
    autonumber
    participant Processes
    participant Payments
    participant Authz
    participant Provider as Stripe или local sandbox
    participant DB as payments-db
    participant Broker as Redpanda

    Processes->>Payments: gRPC AttemptPayment(auction, round, candidate, amount, key)
    Payments->>Authz: operate_payments(workload, auction)
    Payments->>DB: PaymentAttemptStarted + outbox (transaction)
    Payments->>Provider: Charge(settlement:auction:round)
    Provider-->>Payments: Succeeded | RecoveryRequired | Processing
    Payments->>DB: attempt state + attempt outbox (transaction)
    DB-->>Broker: Debezium → gavel.payments.attempts.v1
    Broker-->>Processes: delivery-safe envelope without provider data or recovery URL
    Processes->>Payments: GetSettlementAttempt(expected auction + round)
    Payments-->>Processes: authoritative status; recovery URL only for current recovery state
    alt recovery completed
        Processes-->>Payments: wait for ContinuePayment from participant
        Payments->>Provider: ContinuePayment(same operation ref)
        Provider-->>Payments: Succeeded
    else recovery deadline reached
        Processes->>Payments: ExpirePayment(attempt, key)
        Payments->>Provider: QueryOrCancel(same operation ref)
        Provider-->>Payments: Succeeded or Declined
        Payments->>DB: PaymentSucceeded or PaymentExpired + outbox
    end
```

## Local outcome control

Sandbox выбирается только в local runtime profile. Seed или smoke устанавливает исход через защищённый control route для конкретных `auction_id` и `round`:

- `success` немедленно возвращает provider-neutral `Succeeded`;
- `recovery_then_success` сначала возвращает `RecoveryRequired`, а `ContinuePayment` завершает ту же operation;
- `expire` остаётся в recovery до owner-команды `ExpirePayment`, после которой provider query возвращает нейтральный decline и Payments фиксирует `Expired`.

Подтверждение sandbox-инструмента проходит через тот же `VerifiedProviderEvent`, inbox и transactional `PaymentInstrumentReady`, что и подписанный Stripe callback после его отображения адаптером. Domain/application слой не проверяет имя адаптера и не содержит sandbox-ветвей.

## Privacy и повторы

- Stable provider key и operation ref делают неопределённый transport outcome повторяемым.
- Process RPC требует trusted Processes identity, bounded deadline и Authz permission.
- Broker envelope содержит event/aggregate identity, versions, occurred-at, correlation и causation metadata, но не содержит provider refs, provider objects или recovery URL.
- Recovery URL доступен только через authoritative `GetSettlementAttempt` и не записывается в broker event.
