# auction-lifecycle-settlement Specification

## Purpose

Определяет authoritative lifecycle и settlement аукциона, где Processes управляет временем и повторами, а Auction единолично принимает доменные решения.

## Requirements

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

### Requirement: Lifecycle workflow имеет стабильную идентичность
Processes SHALL поддерживать не более одного durable lifecycle workflow на `auction_id`; повторные, запоздалые и переупорядоченные асинхронные owner events MUST обрабатываться по `event_id` и возрастающей aggregate version без отката уже принятого состояния.

#### Scenario: AuctionOpened доставлен повторно
- **WHEN** Auction публикует `AuctionOpened`, а subscriber повторно передаёт тот же event в Processes
- **THEN** Processes запускает либо сигнализирует workflow со стабильным ID `auction-lifecycle/{auction_id}`
- **THEN** второй workflow и второй closing timer не создаются

#### Scenario: Deadline event пришёл не по порядку
- **WHEN** Processes уже принял Auction event с большей aggregate version, а затем получает более старый `DeadlineExtended` или `AuctionRescheduled`
- **THEN** workflow игнорирует устаревший дедлайн и сохраняет более новую authoritative версию

### Requirement: Lifecycle workflow следует последнему дедлайну Auction
Processes SHALL держать durable timer на последнем versioned `ends_at`, полученном из асинхронных событий Auction, и после таймера SHALL синхронно вызвать owner-команду `CloseAuction` со стабильным idempotency key.

#### Scenario: Дедлайн продлён до срабатывания таймера
- **WHEN** Auction публикует более новый `DeadlineExtended` или `AuctionRescheduled`
- **THEN** Processes отменяет логическое ожидание старого дедлайна и ждёт новый `ends_at`
- **THEN** `CloseAuction` не вызывается по устаревшему времени

#### Scenario: Auction возвращает too_early
- **WHEN** timer срабатывает, но синхронная команда Auction возвращает typed `too_early` с актуальным дедлайном
- **THEN** Processes не принимает решение о закрытии и переставляет durable timer на authoritative deadline

#### Scenario: Auction уже завершён
- **WHEN** повторная `CloseAuction` сообщает уже зафиксированный `SOLD`, `UNSOLD` или `CANCELLED` outcome
- **THEN** workflow завершает lifecycle без новых доменных команд

### Requirement: Settlement candidate передаётся только по restricted contract
Auction SHALL публиковать `SettlementCandidateSelected` для Processes через отдельный versioned restricted process-event contract с auction, candidate, settlement round и amount; публичный `AuctionPublicEvent` MUST NOT содержать candidate identity, секретные максимумы или внутренние storage keys.

#### Scenario: Auction выбирает первого или следующего кандидата
- **WHEN** authoritative агрегат Auction фиксирует `SettlementCandidateSelected`
- **THEN** тот же commit создаёт restricted integration event, достаточный для запуска соответствующего settlement round
- **THEN** публичный event contract остаётся privacy-safe и не раскрывает candidate identity или максимум

#### Scenario: Restricted event повторён
- **WHEN** один process event доставлен Processes несколько раз
- **THEN** workflow принимает round ровно один раз по event id и Auction aggregate version

### Requirement: Settlement workflow исполняет authoritative round
Processes SHALL поддерживать один `SettleAuction` workflow на `auction_id`, принимать только возрастающий round от Auction и синхронно вызывать Payments с неизменяемыми candidate и amount из restricted owner event.

#### Scenario: Получен новый settlement candidate
- **WHEN** Auction публикует restricted `SettlementCandidateSelected` для round N
- **THEN** Processes вызывает owner-команду Payments `AttemptPayment` со стабильным ключом `settle/{auction_id}:attempt-payment:{N}`
- **THEN** Processes не пересчитывает candidate, amount или closing price

#### Scenario: Запоздалый round получен после продвижения
- **WHEN** workflow уже обрабатывает round N+1 и получает событие либо payment outcome для round N
- **THEN** Processes игнорирует его как stale и не вызывает команд для текущего кандидата

### Requirement: Асинхронный payment outcome восстанавливается у владельца
После асинхронного Payments event Processes SHALL синхронно читать authoritative состояние той же попытки через owner RPC; recovery URL и provider-specific объекты MUST NOT публиковаться в брокерных событиях или сохраняться в доступном для запросов workflow state.

#### Scenario: Callback перевёл попытку в recovery
- **WHEN** Payments публикует `PaymentRecoveryRequired` после асинхронного provider callback
- **THEN** Processes читает attempt по trusted process RPC и сверяет auction, round, attempt version и status
- **THEN** recovery delivery data передаётся только в Notify owner RPC и не попадает в публичный event или telemetry

#### Scenario: Event противоречит authoritative read
- **WHEN** event устарел, а Payments owner RPC возвращает более новый terminal status
- **THEN** Processes продолжает по более новому authoritative outcome и не откатывает attempt state

### Requirement: Recovery window имеет детерминированный terminal исход
При authoritative `RECOVERY_REQUIRED` Processes SHALL запустить двухчасовой durable timer; `SUCCEEDED` до подтверждённого expiry MUST привести к `ConfirmWinner`, а переход к следующему кандидату разрешён только после authoritative `EXPIRED`.

#### Scenario: Кандидат восстанавливает оплату вовремя
- **WHEN** Payments подтверждает `SUCCEEDED` до terminal expiry текущего round
- **THEN** Processes отменяет логическое ожидание recovery timer и вызывает Auction `ConfirmWinner` для текущих candidate и payment
- **THEN** workflow завершается после authoritative подтверждения Auction

#### Scenario: Recovery timer истёк
- **WHEN** два часа истекли без подтверждённого успеха
- **THEN** Processes вызывает Payments `ExpirePayment` с тем же attempt и стабильным idempotency key
- **THEN** Processes вызывает Auction `ForfeitSettlementCandidate` только после authoritative `EXPIRED`

#### Scenario: Успех гоняется с expiry
- **WHEN** `PaymentSucceeded` и истечение recovery timer происходят конкурентно
- **THEN** Processes сверяет terminal outcome у Payments и не forfeits кандидата при authoritative успехе

### Requirement: Settlement сходится к одному terminal состоянию
Processes SHALL продолжать rounds до authoritative `WinnerConfirmed` либо `AuctionUnsold` и MUST не завершать workflow, оставляя текущую payment attempt в non-terminal `PROCESSING`.

#### Scenario: Forfeiture выбрал следующего кандидата
- **WHEN** Auction принимает `ForfeitSettlementCandidate` и публикует новый restricted round
- **THEN** тот же workflow начинает следующий round после version-aware сигнала
- **THEN** closing price не изменяется в Processes

#### Scenario: Кандидаты закончились
- **WHEN** Auction возвращает или публикует authoritative `UNSOLD`
- **THEN** settlement workflow завершается без новых payment attempts

#### Scenario: Worker перезапущен во время ожидания
- **WHEN** Processes worker останавливается во время deadline, recovery timer или ожидания payment outcome
- **THEN** после восстановления workflow продолжает с сохранённой durable history без дублирования owner effects

### Requirement: Повторы activity ограничены и идемпотентны
Processes MUST задавать bounded deadline каждому owner RPC, повторять только transient transport failures и использовать ключи, стабильные между activity retry, workflow replay и worker restart; typed domain rejection SHALL обрабатываться как workflow outcome.

#### Scenario: Ответ потерян после owner commit
- **WHEN** owner команда committed, но Processes получает timeout
- **THEN** activity повторяет неизменённую команду с тем же idempotency key
- **THEN** owner возвращает сохранённый результат без второго эффекта

#### Scenario: Owner возвращает domain rejection
- **WHEN** Auction, Payments или Notify возвращает typed non-retryable rejection
- **THEN** Processes не применяет автоматический transport retry
- **THEN** workflow обрабатывает rejection по явной ветке либо завершает failure с безопасной причиной

### Requirement: Уведомления не меняют доменный исход
Processes SHALL запрашивать у Notify settlement/recovery/winner уведомления с детерминированным notification id; недоступность или отказ Notify MUST NOT продлевать recovery deadline, менять payment status или выбирать победителя.

#### Scenario: Notify временно недоступен
- **WHEN** Notify RPC возвращает transient failure
- **THEN** Processes безопасно повторяет тот же notification intent по policy
- **THEN** доменные команды и двухчасовой recovery deadline сохраняют исходную семантику

#### Scenario: Notify окончательно отклоняет уведомление
- **WHEN** Notify возвращает typed non-retryable rejection
- **THEN** Processes фиксирует наблюдаемый delivery failure без изменения authoritative Auction или Payments state
