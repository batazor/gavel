# catalog-lot-lifecycle Specification

## Purpose

Определяет authoritative жизненный цикл карточки лота в Catalog: создание, разрешённое редактирование и снятие с атомарными events и безопасными повторами команд.

## Requirements

### Requirement: Catalog создаёт валидный лот продавца
Catalog SHALL создавать лот только после проверки обязательного описания, денег, closing policy, изображений и permission `platform:create_lot`; торговые инварианты SHALL оставаться в Auction.

#### Scenario: Продавец выставляет лот
- **WHEN** verified user с permission `create_lot` отправляет допустимый `ListLot` и новый idempotency key
- **THEN** Catalog создаёт лот со стабильным заранее выделенным id и состоянием `LISTED`
- **THEN** result не содержит внутренних receipt/storage/authz значений

#### Scenario: Authz недоступен или возвращает deny
- **WHEN** permission check завершается deny, timeout или dependency failure
- **THEN** Catalog fail closed и не создаёт лот, attachments или outbox event

### Requirement: Grants создаются до публикации лота
Catalog SHALL идемпотентно provision `lot:platform` и `lot:seller` через Authz до локального commit и SHALL сохранить returned revision вместе с лотом.

#### Scenario: Provisioning и commit успешны
- **WHEN** Authz принимает owner-scoped relationship batch
- **THEN** Catalog одной локальной транзакцией сохраняет лот, Authz revision, attachments, command result и `LotListed`

#### Scenario: Локальный commit потерян после provisioning
- **WHEN** relationships записаны, но catalog-db transaction не зафиксирована
- **THEN** retry с тем же idempotency key безопасно завершает создание
- **THEN** reconciliation может удалить подтверждённый orphan grant только при доказанном отсутствии лота

### Requirement: LotListed остаётся совместимым asynchronous contract
Catalog MUST публиковать `LotListed v1` через transactional outbox с полями, необходимыми Auction для идемпотентного открытия торгов, и MUST NOT вызывать Auction синхронно.

#### Scenario: Лот зафиксирован
- **WHEN** Catalog commit создаёт лот версии 1
- **THEN** та же transaction создаёт один outbox envelope с event id, correlation id, lot/auction/seller ids, start price, currency, bid step и closing policy
- **THEN** Auction как authoritative owner торгов получает событие через Redpanda at-least-once

#### Scenario: Outbox доставлен повторно
- **WHEN** один `LotListed` доставляется Auction несколько раз
- **THEN** существующий Auction consumer распознаёт exact duplicate и не открывает второй stream

### Requirement: Редактирование ограничено состоянием Catalog
Catalog SHALL разрешать `UpdateLot` только продавцу с permission `lot:edit`, пока описание не заморожено после первой принятой ставки, и SHALL атомарно публиковать `LotUpdated`.

#### Scenario: Лот ещё не получил ставку
- **WHEN** владелец меняет допустимое title или description до freeze
- **THEN** Catalog увеличивает version и сохраняет `LotUpdated` в той же transaction

#### Scenario: Описание заморожено
- **WHEN** Catalog получил monotonic факт первой ставки для лота
- **THEN** последующий `UpdateLot` возвращает стабильный domain conflict без изменения состояния

### Requirement: Снятие выполняет только авторизованный шаг saga
Catalog SHALL принимать внутренний `WithdrawLot` только от trusted Processes workload с permission `lot:execute_withdraw`; решение об отсутствии ставок и отмена торгов остаются в Auction.

#### Scenario: Processes завершает снятие после Auction cancellation
- **WHEN** Processes вызывает Catalog-owned gRPC command с lot id, causal context и idempotency key
- **THEN** Catalog переводит существующий `LISTED` lot в `WITHDRAWN` и атомарно создаёт `LotWithdrawn`

#### Scenario: Повтор или несовместимый key
- **WHEN** exact command доставлен повторно
- **THEN** Catalog возвращает сохранённую version без второго event
- **WHEN** тот же key используется для другого lot/payload
- **THEN** Catalog возвращает typed idempotency conflict без изменения состояния

### Requirement: Catalog commands имеют durable idempotency
Каждая изменяющая команда SHALL durable фиксировать canonical payload и authoritative result по caller/idempotency key в catalog-db.

#### Scenario: Response потерян после commit
- **WHEN** command зафиксирован, но caller не получил response
- **THEN** exact retry возвращает прежний result и не повторяет outbox/domain effect

#### Scenario: Конкурентные команды изменяют одну version
- **WHEN** два допустимых command одновременно ожидают одну aggregate version
- **THEN** не более одного commit изменяет lot, а второй получает replay или retryable optimistic conflict

### Requirement: Catalog lifecycle telemetry не раскрывает сущности
Catalog SHALL публиковать bounded command/outcome, dependency и outbox metrics и MUST NOT использовать lot/seller/image ids, idempotency keys, revision tokens или credentials как labels/log fields.

#### Scenario: Команда завершается domain rejection
- **WHEN** create, update или withdraw отклоняется ожидаемым инвариантом
- **THEN** metric отличает bounded command и rejection outcome от retryable dependency failure
- **THEN** safe error не раскрывает входные identifiers
