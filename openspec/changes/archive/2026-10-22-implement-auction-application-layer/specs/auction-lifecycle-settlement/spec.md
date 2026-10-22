# Spec Delta

## Purpose

Определяет authoritative lifecycle и settlement аукциона, где Processes управляет временем и повторами, а Auction единолично принимает доменные решения.

## ADDED Requirements

### Requirement: Lifecycle-команды исполняет владелец Auction
Auction SHALL принимать versioned gRPC команды `CloseAuction`, `CancelAuction`, `ForfeitSettlementCandidate` и `ConfirmWinner` только от разрешённой workload identity и SHALL сам проверять состояние агрегата.

#### Scenario: Processes вызывает разрешённую команду
- **WHEN** mTLS identity соответствует `service_account:processes` и Authz разрешает требуемое service permission
- **THEN** Auction загружает authoritative stream и применяет соответствующую доменную команду
- **THEN** Processes получает typed authoritative outcome, а не принимает доменное решение самостоятельно

#### Scenario: Caller подменяет principal в payload
- **WHEN** payload содержит идентификатор, не подтверждённый trusted workload identity или состоянием агрегата
- **THEN** Auction не использует его как доказательство полномочий
- **THEN** команда отклоняется без изменения состояния

### Requirement: Закрытие следует authoritative дедлайну
Auction SHALL закрыть торги только когда переданное `now` не меньше текущего `endsAt`; повтор уже успешного закрытия SHALL возвращать эквивалентный результат без новых событий.

#### Scenario: Таймер сработал на актуальном дедлайне
- **WHEN** Processes вызывает `CloseAuction` с `now >= endsAt` для открытого аукциона
- **THEN** Auction фиксирует `AuctionClosed` и выбирает первого settlement candidate либо фиксирует `AuctionUnsold`
- **THEN** terminal факты попадают в outbox атомарно с private events

#### Scenario: Дедлайн был продлён
- **WHEN** `CloseAuction` приходит с `now < endsAt`
- **THEN** Auction возвращает typed `too_early` с актуальным состоянием без событий
- **THEN** Processes может переставить durable timer

#### Scenario: Закрытие повторено
- **WHEN** тот же intent закрытия доставлен после успешного commit
- **THEN** Auction возвращает ранее зафиксированный outcome и version
- **THEN** второй `AuctionClosed` не создаётся

### Requirement: Отмена имеет один terminal outcome
Auction SHALL авторизовать отмену, применить её только в допустимом состоянии и атомарно зафиксировать `AuctionCancelled` с idempotent result.

#### Scenario: Разрешённая отмена
- **WHEN** Processes вызывает `CancelAuction` с разрешённой identity и новым idempotency key
- **THEN** Auction фиксирует отмену и возвращает terminal version
- **THEN** публичный факт создаётся в той же transaction

#### Scenario: Проданный аукцион отменить нельзя
- **WHEN** команда отмены приходит после подтверждения победителя
- **THEN** Auction возвращает typed domain rejection без новых событий

### Requirement: Forfeiture выбирает следующего кандидата
Auction SHALL исключить только текущего settlement candidate для ожидаемого settlement round и заново рассчитать следующего кандидата по оставшимся максимумам, не изменяя closing price торгов.

#### Scenario: Текущий кандидат не оплатил
- **WHEN** Processes вызывает forfeiture для текущих candidate и round
- **THEN** Auction фиксирует forfeiture и выбирает следующего кандидата с новым settlement amount
- **THEN** ответ содержит новый status, settlement round и stream version без раскрытия чужих максимумов

#### Scenario: Кандидаты закончились
- **WHEN** после forfeiture не остаётся допустимых участников
- **THEN** Auction фиксирует `AuctionUnsold` как terminal outcome

#### Scenario: Устаревший settlement round
- **WHEN** повтор или запоздалый вызов относится не к текущему candidate/round
- **THEN** Auction возвращает typed stale-candidate rejection без изменения stream

### Requirement: Победитель подтверждается после authoritative оплаты
Auction SHALL подтвердить только текущего settlement candidate по команде с идентификатором успешного платежа и SHALL гарантировать не более одного победителя.

#### Scenario: Оплата текущего кандидата успешна
- **WHEN** Processes после authoritative факта Payments вызывает `ConfirmWinner` для текущих candidate и payment
- **THEN** Auction фиксирует `WinnerConfirmed` и status `SOLD`
- **THEN** public outbox содержит безопасный terminal факт без provider-specific payment object

#### Scenario: Подтверждается другой участник
- **WHEN** candidate в команде не совпадает с текущим settlement candidate
- **THEN** Auction возвращает typed candidate mismatch без изменения stream

### Requirement: Lifecycle retry не дублирует эффекты
Каждая lifecycle-команда MUST использовать стабильный idempotency key, а Auction SHALL фиксировать payload fingerprint, typed result, stream append, projection и outbox в одной transaction.

#### Scenario: Response потерян после commit
- **WHEN** Processes повторяет тот же intent после transport timeout
- **THEN** Auction возвращает сохранённый typed result
- **THEN** private stream, projection и outbox остаются без дублей

#### Scenario: Повторный ключ имеет другой payload
- **WHEN** idempotency key повторно используется для другой команды, candidate или round
- **THEN** Auction возвращает conflict и не выполняет новую команду
