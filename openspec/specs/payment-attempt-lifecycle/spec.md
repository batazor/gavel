# payment-attempt-lifecycle Specification

## Purpose

Определяет provider-neutral платёжную попытку settlement candidate, её восстановление и terminal исходы без переноса выбора победителя или таймеров в Payments.

## Requirements

### Requirement: Попытку создаёт только settlement workflow
Payments SHALL принимать `AttemptPayment` только от trusted Processes workload с permission `payment_attempt:process`, bounded deadline, causal context и idempotency key.

#### Scenario: Processes начинает оплату кандидата
- **WHEN** Processes передаёт auction id, settlement round, candidate id и положительную сумму в поддерживаемой currency
- **THEN** Payments создаёт одну попытку для `(auction_id, settlement_round)` и вызывает provider-neutral charge
- **THEN** Payments не пересчитывает сумму и не ранжирует кандидатов

#### Scenario: Неподтверждённый caller или недоступный Authz
- **WHEN** caller не является trusted Processes workload либо permission нельзя authoritative подтвердить
- **THEN** Payments fail closed до provider call и domain mutation

### Requirement: Платёжная попытка имеет нейтральный lifecycle
Payments SHALL отображать provider outcomes только в `PROCESSING`, `RECOVERY_REQUIRED`, `SUCCEEDED` или `EXPIRED` и MUST NOT раскрывать provider-specific объекты или статусы наружу.

#### Scenario: Списание завершилось синхронно
- **WHEN** provider подтверждает успешное списание
- **THEN** Payments фиксирует `SUCCEEDED` и атомарно создаёт `PaymentSucceeded`

#### Scenario: Требуется участие кандидата
- **WHEN** provider сообщает нейтральный recoverable outcome
- **THEN** Payments фиксирует `RECOVERY_REQUIRED` и атомарно создаёт `PaymentRecoveryRequired`
- **THEN** recovery URL доступен только авторизованному кандидату и не публикуется в domain event

#### Scenario: Исход пока неизвестен
- **WHEN** provider принял операцию, но не вернул terminal outcome
- **THEN** Payments фиксирует `PROCESSING` и ожидает проверенный callback или безопасное продолжение той же попытки

### Requirement: На аукцион приходится не более одного успеха
Payments MUST гарантировать не более одного `PaymentSucceeded` для auction id независимо от числа settlement rounds, повторов команд и конкурентных callbacks.

#### Scenario: Более поздний round пытается завершиться успехом
- **WHEN** для auction id уже зафиксирован успешный payment attempt
- **THEN** второй успех отклоняется без второго `PaymentSucceeded`
- **THEN** существующий authoritative success остаётся неизменным

#### Scenario: Два исхода конкурируют
- **WHEN** concurrent callbacks пытаются успешно завершить разные attempts одного auction
- **THEN** database constraint и transaction позволяют commit ровно одному успеху

### Requirement: Команды попытки имеют durable idempotency
Payments SHALL durable фиксировать canonical payload и authoritative result по trusted caller/idempotency key; provider key SHALL стабильно выводиться из auction id и settlement round.

#### Scenario: Response потерян после commit
- **WHEN** команда завершилась, но Processes не получил response
- **THEN** exact retry возвращает сохранённый result без новой попытки, provider call или outbox event

#### Scenario: Ключ повторно использован с другим payload
- **WHEN** тот же caller использует key для другого auction, round, candidate или amount
- **THEN** Payments возвращает typed idempotency conflict без изменения состояния

#### Scenario: Конкурентные exact retries
- **WHEN** несколько instances одновременно обрабатывают одинаковую команду
- **THEN** один durable claim выполняет effect, а остальные воспроизводят результат или получают bounded retryable outcome

### Requirement: Кандидат продолжает только собственную попытку
Payments SHALL разрешать `ContinuePayment` только кандидату с permission `payment_attempt:continue_payment` для своей `RECOVERY_REQUIRED` попытки и SHALL продолжать тот же provider payment intent.

#### Scenario: Кандидат восстанавливает оплату
- **WHEN** владелец попытки вызывает continue до expiry
- **THEN** Payments возвращает безопасный provider-hosted recovery URL либо уже terminal authoritative result

#### Scenario: Чужая или terminal попытка
- **WHEN** другой principal либо владелец уже истёкшей попытки вызывает continue
- **THEN** Payments возвращает stable domain rejection без provider effect

### Requirement: Expiry согласован с конкурентным успехом
Payments SHALL принимать `ExpirePayment` только от Processes и SHALL перед переходом к следующему кандидату подтвердить terminal outcome либо безопасно отменить provider operation.

#### Scenario: Recovery window истекло раньше успеха
- **WHEN** Processes вызывает expiry, а provider operation может быть отменена и успех не зафиксирован
- **THEN** Payments фиксирует `EXPIRED` и атомарно создаёт `PaymentExpired`

#### Scenario: Успех конкурирует с expiry
- **WHEN** проверенный provider success зафиксирован до commit expiry
- **THEN** success побеждает и `ExpirePayment` возвращает authoritative typed rejection/result без `PaymentExpired`

#### Scenario: Исход нельзя безопасно подтвердить
- **WHEN** provider недоступен и attempt остаётся потенциально chargeable
- **THEN** Payments возвращает retryable failure и не подтверждает expiry

### Requirement: Attempt events остаются минимальными и совместимыми
Payments MUST публиковать versioned `PaymentAttemptStarted`, `PaymentRecoveryRequired`, `PaymentSucceeded` и `PaymentExpired` через transactional outbox без provider objects и secret payment data.

#### Scenario: Lifecycle transition committed
- **WHEN** attempt меняет externally relevant state
- **THEN** та же transaction сохраняет state/version, command result и один public event
- **THEN** Processes получает asynchronous факт и самостоятельно управляет timer/следующим шагом

### Requirement: Attempt telemetry имеет bounded cardinality
Payments SHALL наблюдать command, provider operation, callback, transition и outbox outcomes без auction/candidate/attempt ids, amounts, idempotency keys, recovery URLs или provider refs в labels.

#### Scenario: Provider недоступен
- **WHEN** charge, continue или cancel завершается timeout
- **THEN** metric различает bounded operation и retryable dependency outcome
- **THEN** safe error и logs не раскрывают платёжные identifiers или credentials
