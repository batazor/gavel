# Завершить расчёт по закрытому аукциону

Владелец процесса: Processes · Участники: Auction, Payments, settlement candidate, время · Диаграмма: `flow.c4` → `flow_payment`

## Цель

После закрытия торгов автоматически получить оплату от первого подходящего участника. Если платёж требует участия пользователя, дать текущему кандидату два часа на восстановление; затем исключить его и повторить процесс со следующим кандидатом.

Temporal владеет последовательностью, таймерами и повторами. Auction владеет кандидатом и суммой. Payments владеет платёжной попыткой и скрывает конкретного провайдера. Notify владеет шаблоном, каналом и delivery retry.

## Термины

- **Settlement candidate** — участник, которому сейчас предлагается купить лот. Это ещё не подтверждённый победитель.
- **Winner** — кандидат, чья оплата успешно завершилась и подтверждена Auction.
- **Settlement round** — номер кандидата в процессе; входит в ключ идемпотентности платежа.
- **Recovery window** — двухчасовой Temporal timer, в течение которого кандидат может продолжить оплату или заменить инструмент.

## Основной сценарий

```mermaid
sequenceDiagram
    autonumber
    participant Auction
    participant Broker as Redpanda
    participant Workflow as SettleAuction
    participant Notify
    participant Payments
    participant Provider as Payment provider
    participant BFF
    participant Web

    Auction->>Broker: Restricted SettlementCandidateSelected<br/>(auction, candidate, amount, round=1)
    Broker-->>Workflow: Start or signal workflow<br/>from ACL-protected process topic
    Workflow-->>Broker: Durable signal accepted<br/>consumer commits offset
    Workflow->>Notify: gRPC SendNotification<br/>(deterministic notification_id)
    Workflow->>Payments: gRPC AttemptPayment(auction, round, user, amount)
    Payments->>Payments: Commit one attempt per auction + round<br/>and PaymentAttemptStarted before provider call
    Payments->>Provider: Charge<br/>idempotency-key = settlement:{auction}:{round}
    Provider-->>Payments: Succeeded synchronously or by callback
    Payments->>Broker: PaymentSucceeded
    Broker-->>Workflow: owner_event(PaymentChanged version)
    Workflow->>Payments: GetSettlementAttempt(expected auction + round)
    Workflow->>Auction: gRPC ConfirmWinner(user, payment_id)
    Auction->>Broker: WinnerConfirmed
    Workflow->>Notify: gRPC SendNotification<br/>(winner confirmed)
    Web->>BFF: Poll LotWorkspace(version-aware)
    BFF->>Auction: Read public terminal state
    Auction-->>BFF: SOLD + stream version
    BFF-->>Web: Terminal workspace
    Workflow->>Workflow: Complete
```

## Recovery window

Если Payments публикует `PaymentRecoveryRequired`:

```mermaid
sequenceDiagram
    autonumber
    actor Candidate as Settlement candidate
    participant Workflow as SettleAuction
    participant Broker as Redpanda
    participant Notify
    participant Web
    participant Payments
    participant Provider as Payment provider
    participant Auction

    Payments->>Broker: PaymentRecoveryRequired
    Broker-->>Workflow: owner_event(PaymentChanged version)
    Workflow-->>Broker: Durable signal accepted<br/>consumer commits offset
    Workflow->>Payments: GetSettlementAttempt → RecoveryRequired
    Workflow->>Notify: notify_recovery<br/>(activity fetches URL; history gets redacted ack)
    Notify-->>Candidate: Recovery email
    Workflow->>Workflow: Start durable 2-hour timer
    alt Candidate recovers before deadline
        Candidate->>Web: Open recovery URL
        Web->>Payments: ContinuePayment(attemptId)
        Payments->>Provider: Continue same payment attempt
        Provider-->>Payments: PaymentSucceeded
        Payments->>Broker: PaymentSucceeded
        Broker-->>Workflow: owner_event(PaymentChanged version)
        Workflow->>Payments: GetSettlementAttempt → Succeeded
        Workflow->>Workflow: Cancel timer
        Workflow->>Auction: ConfirmWinner
    else Timer expires first
        Workflow->>Payments: GetSettlementAttempt before expiry
        Workflow->>Payments: gRPC ExpirePayment
        Payments->>Provider: Cancel if needed
        Payments->>Broker: PaymentExpired
        Broker-->>Workflow: owner_event(PaymentChanged version)
        Workflow->>Auction: gRPC ForfeitSettlementCandidate(user, round)
        Auction->>Auction: Exclude candidate and recalculate<br/>without changing closing price
        alt Another candidate exists
            Auction->>Broker: Restricted SettlementCandidateSelected<br/>(next, new amount, next round)
            Broker-->>Workflow: owner_event(AuctionChanged version)
            Workflow->>Workflow: Repeat settlement cycle
        else No candidates remain
            Auction->>Broker: AuctionUnsold
            Broker-->>Workflow: owner_event(AuctionChanged version)
            Workflow->>Workflow: Complete
        end
    end
```

## Расчёт settlement amount следующего кандидата

Settlement amount не наследуется от выбывшего кандидата. Auction удаляет его максимум из расчёта и применяет обычную pricing policy к оставшимся ставкам. Поэтому сумма нового кандидата не превышает его максимум. Публичная closing price остаётся частью неизменяемой истории торгов.

Пример: A max 200, B max 150, C max 100, шаг 10. Изначально A платил бы 160. После forfeiture A кандидат B получает цену 110, а не 160.

## Гонки и ошибки

- Повтор activity не создаёт новую попытку: ключ `settlement:{auction_id}:{round}`.
- Дубли callback игнорируются по `provider_event_ref`.
- Успех одновременно с expiry: `ExpirePayment` сначала делает provider query/cancel; подтверждённый успех побеждает. Оптимистическая версия допускает только один terminal commit/outbox event.
- Неопределённый query/cancel не переводит попытку в `Expired`: тот же idempotency receipt освобождается для безопасного повтора.
- Попытку в `Processing` нельзя бросить: workflow ждёт terminal outcome и не переходит к следующему кандидату.
- Провайдер недоступен: activity повторяется с backoff; recovery window начинается только после `PaymentRecoveryRequired`.
- Email не доставлен: recovery URL остаётся на странице лота; timer не продлевается.
- После подтверждённого Auction forfeiture workflow best-effort отправляет выбывшему кандидату отдельное уведомление; Notify получает только безопасные template data, включая `auction_id`, без recovery URL или provider reference.
- Restart worker не создаёт новый owner effect: ключи зависят от auction + intent + round, но не от Temporal run id.
- URL восстановления, provider reference и bidder maxima не сохраняются в workflow input/state/history, query, search attributes, logs или metric labels.

## Runtime contract

Workflow type `SettleAuction` работает на `settle-auction-v1`; все owner-события приходят одним version-aware signal `owner_event`. Production Redpanda consumer использует `signal-with-start`, а offset подтверждает только после успешного сигнала или durable quarantine. Два часа измеряются Temporal timer-ом. При гонке signal/timer workflow всегда перечитывает authoritative attempt у Payments и только после этого выбирает confirm или expiry. Контрактные и e2e fixtures фиксируют безопасное поведение при duplicate/reordered event и lost RPC response.

Runtime считается ready только когда доступны Temporal, public/restricted/payments/quarantine topics, consumer loop и Auction/Payments/Notify RPC channels. Потеря email delivery не меняет settlement outcome, но недоступный Notify RPC снимает новый pod из readiness до начала обработки.

## Что не делаем сейчас

Возвраты, выплаты продавцу, штраф за неуспешную оплату, повторный допуск выбывшего кандидата.
