# auction-bidding Specification

## Purpose

Определяет безопасное открытие торгов, постановку proxy bid и изменение дедлайна при конкурентных запросах, повторах доставки и межконтекстных проверках.

## Requirements

### Requirement: Торги открываются из authoritative факта Catalog
Auction SHALL асинхронно открывать один аукцион на лот после versioned события `LotListed`, которым владеет Catalog, и SHALL безопасно игнорировать повторную доставку того же факта.

#### Scenario: Новый лот выставлен
- **WHEN** Auction получает `LotListed` с идентификатором лота, продавцом, стартовой ценой, шагом и closing policy
- **THEN** Auction создаёт event stream и authoritative публичную проекцию открытых торгов
- **THEN** Auction публикует собственные факты из transactional outbox только после commit

#### Scenario: Событие открытия доставлено повторно
- **WHEN** тот же `LotListed` приходит повторно
- **THEN** Auction не создаёт второй stream и не публикует дубликаты бизнес-фактов
- **THEN** consumer подтверждает уже обработанное событие

### Requirement: Ставка проверяется владельцем Auction
Auction SHALL принимать `placeBid` только для аутентифицированного principal с permission `auction#place_bid`, открытых торгов и актуальной локальной проекции готовности платёжного инструмента из событий Payments.

#### Scenario: Участник размещает допустимый максимум
- **WHEN** Authz разрешает действие, payment readiness активна и максимум выше текущей authoritative цены
- **THEN** Auction принимает ставку и возвращает новую цену, признак немедленного outbid и принятую версию
- **THEN** ответ не раскрывает максимумы или private identity других участников

#### Scenario: Authz недоступен
- **WHEN** authoritative permission check завершается timeout или ошибкой
- **THEN** Auction отклоняет mutation без доменных событий
- **THEN** отказ обозначен как retryable dependency failure

#### Scenario: Платёжный инструмент не готов
- **WHEN** локальная проекция Payments не содержит актуального `PaymentInstrumentReady` для участника
- **THEN** Auction возвращает стабильный `payment_method_required` без изменения stream

### Requirement: Повтор ставки имеет один бизнес-эффект
Каждая mutation Auction MUST принимать idempotency key; повтор с теми же principal и payload SHALL возвращать ранее зафиксированный authoritative result, а повтор ключа с другим payload SHALL завершаться конфликтом.

#### Scenario: Клиент повторяет принятую ставку
- **WHEN** после неопределённого transport outcome клиент повторяет тот же `placeBid`
- **THEN** Auction возвращает тот же result и accepted version
- **THEN** новый private event и новый public outbox event не создаются

#### Scenario: Ключ переиспользован для другой ставки
- **WHEN** тот же principal передаёт прежний idempotency key с другим auction или maximum
- **THEN** Auction возвращает idempotency conflict без изменения состояния

### Requirement: Конкурентные ставки сериализуются по версии stream
Auction MUST принимать изменение только относительно ожидаемой версии stream и SHALL повторить decision на свежем authoritative state при optimistic conflict в пределах ограниченного retry budget.

#### Scenario: Две ставки приходят одновременно
- **WHEN** два запроса загружают одну версию аукциона и первый commit побеждает
- **THEN** второй запрос повторно загружает stream и заново проверяет свой максимум
- **THEN** клиент получает результат, соответствующий фактическому порядку commit

#### Scenario: Конфликт не разрешён в retry budget
- **WHEN** stream продолжает меняться до исчерпания ограниченного retry budget
- **THEN** Auction возвращает retryable contention failure без неподтверждённого результата

### Requirement: Принятая ставка публикует только безопасные факты
В одной transaction с private `MaxBidPlaced` Auction SHALL зафиксировать `BidRecorded`, а при изменении цены или дедлайна — соответствующие публичные события; ни одно публичное событие MUST NOT содержать maximum, `user_id` или private leader identity.

#### Scenario: Ставка меняет цену и дедлайн
- **WHEN** proxy pricing принимает ставку и closing policy продлевает торги
- **THEN** outbox содержит versioned `BidRecorded`, `PriceChanged` и `DeadlineExtended`
- **THEN** все события несут только auction-scoped alias и публичное состояние

#### Scenario: Лидер повышает секретный максимум без изменения цены
- **WHEN** принятая ставка не меняет публичную цену
- **THEN** outbox содержит `BidRecorded`, но не содержит `PriceChanged`
- **THEN** новый maximum остаётся только в private event stream

### Requirement: Auction фиксирует потерю лидерства как restricted факт
После каждой успешно принятой ставки Auction SHALL определить authoritative лидера до и после решения и атомарно с private stream создать один restricted `BidderOutbid`, если участник потерял лидерство либо новая ставка немедленно осталась не-лидирующей. Событие MUST NOT публиковаться в public topic и MUST NOT содержать maximum или identity лидера.

#### Scenario: Новый максимум меняет лидера
- **WHEN** принятая ставка одного участника делает его новым authoritative лидером вместо другого участника
- **THEN** владелец Auction фиксирует один asynchronous restricted `BidderOutbid` для прежнего лидера
- **THEN** событие содержит recipient, auction id, причинивший переход bid id и aggregate version, но не содержит maximum или identity нового лидера

#### Scenario: Proxy maximum немедленно перебивает новую ставку
- **WHEN** ставка принята, но после proxy pricing поставивший её участник не становится authoritative лидером
- **THEN** владелец Auction фиксирует один asynchronous restricted `BidderOutbid` для этого участника
- **THEN** public `BidRecorded` и `PriceChanged` сохраняют прежние privacy-гарантии

#### Scenario: Текущий лидер повышает собственный maximum
- **WHEN** принятая ставка текущего лидера не приводит к потере лидерства ни одним участником
- **THEN** Auction не создаёт `BidderOutbid`

#### Scenario: Повтор команды ставки
- **WHEN** idempotent replay возвращает ранее зафиксированный результат `placeBid`
- **THEN** Auction не создаёт второй private факт или restricted outbox event

### Requirement: Продавец может перенести открытый дедлайн
Auction SHALL выполнить `rescheduleAuction` только для владельца лота с permission `auction#reschedule`, будущим `scheduledEnd` и допустимым состоянием closing policy.

#### Scenario: Продавец переносит дедлайн
- **WHEN** авторизованный продавец задаёт допустимое будущее время для открытого аукциона
- **THEN** Auction атомарно фиксирует новый `endsAt` и outbox-событие `AuctionRescheduled`
- **THEN** mutation возвращает authoritative новую версию аукциона

#### Scenario: Другой principal пытается перенести дедлайн
- **WHEN** principal не является продавцом или не имеет permission
- **THEN** Auction возвращает forbidden без изменения stream
