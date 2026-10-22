# Spec Delta

## ADDED Requirements

### Requirement: Payments settlement commands принадлежат Payments
Payments SHALL владеть additive `gavel.payments.v1.AttemptPayment` и `ExpirePayment`, принимать их только от trusted Processes workload с deadline и idempotency key и возвращать provider-neutral authoritative outcome.

#### Scenario: Processes начинает попытку оплаты
- **WHEN** Processes вызывает `AttemptPayment` с authoritative auction, round, candidate и amount
- **THEN** Payments проверяет permission `payment_attempt:process` и возвращает stable attempt id/status либо typed domain rejection
- **THEN** дальнейшие lifecycle facts публикуются отдельно через Payments outbox

#### Scenario: Processes завершает recovery window
- **WHEN** Processes вызывает `ExpirePayment` для существующей попытки
- **THEN** Payments подтверждает отсутствие success и безопасный cancel до authoritative `EXPIRED`
- **THEN** неопределённый provider outcome возвращается как retryable failure, а не ложный expiry

#### Scenario: Неподтверждённый workload вызывает payment command
- **WHEN** caller не имеет trusted mesh identity Processes либо пытается передать identity в metadata/payload
- **THEN** Payments возвращает unauthenticated/permission-denied до provider или domain effect

#### Scenario: Processes повторяет command после timeout
- **WHEN** exact payment command повторён с тем же idempotency key
- **THEN** Payments возвращает сохранённый authoritative result без второго provider call, transition или outbox event
- **THEN** тот же key с другим payload отклоняется как typed conflict

### Requirement: Payments RPC распространяется как versioned Generated SDK
Payments MUST публиковать owner module `buf.build/gavel/payments` в BSR, а Rust consumers SHALL использовать Generated Cargo SDK с точной версией module commit и plugin вместо локальной компиляции или копирования Payments `.proto`.

#### Scenario: Owner публикует совместимый contract
- **WHEN** Payments меняет RPC schema additive и change проходит lint/build/breaking checks
- **THEN** schema публикуется в BSR под change label до обновления consumers
- **THEN** BSR создаёт Generated Prost/Tonic SDK для того же module commit

#### Scenario: Processes обновляет Payments client
- **WHEN** Processes принимает новую совместимую версию owner contract
- **THEN** его Cargo dependency закрепляет точную BSR SDK version, разрешённую через registry metadata и `Cargo.lock`
- **THEN** Processes не компилирует и не хранит собственную копию Payments schema или generated stubs

#### Scenario: Registry package не соответствует проверенному commit
- **WHEN** SDK version невозможно разрешить к ожидаемому Payments module commit либо package недоступен
- **THEN** consumer build завершается ошибкой вместо fallback на локально сгенерированный или плавающий contract
