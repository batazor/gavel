# outbid-notification-delivery Specification

## Purpose

Определяет безопасную, durable и идемпотентную доставку уведомления участнику, которого authoritative proxy bidding оставил без лидерства в открытом аукционе.

## Requirements

### Requirement: Processes оркестрирует каждый restricted outbid intent
Processes SHALL принимать versioned `BidderOutbid` только из ACL-protected Auction process topic и SHALL запускать отдельный durable notification process, не смешивая его с lifecycle или settlement workflow.

#### Scenario: Restricted факт доставлен впервые
- **WHEN** Processes получает валидный asynchronous `BidderOutbid` от владельца Auction
- **THEN** Processes durable принимает intent до подтверждения broker offset
- **THEN** process запрашивает у Notify доставку только указанному recipient

#### Scenario: Restricted факт доставлен повторно
- **WHEN** broker повторно доставляет тот же owner event
- **THEN** Processes использует тот же workflow/request identity и не создаёт второе логическое уведомление

#### Scenario: Событие пришло из публичного или неизвестного контракта
- **WHEN** outbid payload не соответствует ACL-protected Auction contract, schema version или обязательным полям
- **THEN** Processes fail closed и не вызывает Notify

### Requirement: Outbid notification имеет детерминированную identity
Processes SHALL построить notification id из auction id и authoritative Auction event version; Notify MUST durable дедуплицировать точный повтор, а повтор id с другим recipient, template или data MUST завершить conflict.

#### Scenario: Ответ Notify потерян после commit
- **WHEN** activity повторяет `SendNotification` после неопределённого transport outcome
- **THEN** Notify возвращает сохранённый delivery result без второго email

#### Scenario: Identity повторно использована с другим получателем
- **WHEN** прежний notification id приходит с отличающимся recipient или payload
- **THEN** Notify возвращает typed idempotency conflict без изменения сохранённой команды

### Requirement: Notify доставляет безопасный outbid-template
Notify SHALL поддерживать versioned template `bidder_outbid.v1`, принимающий только recipient и безопасный auction id; message и telemetry MUST NOT содержать maximum, identity другого участника, contact data в labels, provider object или internal storage key.

#### Scenario: Уведомление успешно доставлено
- **WHEN** trusted Processes workload отправляет разрешённый outbid-template с валидными данными
- **THEN** Notify применяет существующие bounded retry и terminal delivery semantics
- **THEN** локальный operator read показывает template, recipient, status и attempts без раскрытия body или template data

#### Scenario: Outbid-template содержит лишние приватные данные
- **WHEN** команда содержит maximum, identity лидера или неподдерживаемый template field
- **THEN** Notify отклоняет команду до channel adapter и не сохраняет доставляемое сообщение

### Requirement: Сбой уведомления не меняет торги
Недоступность Processes, Notify или email channel MUST NOT откатывать принятую ставку, менять Auction leader/price/deadline или блокировать следующие ставки.

#### Scenario: Notify временно недоступен
- **WHEN** ставка и restricted outbox event уже зафиксированы, а Notify RPC временно недоступен
- **THEN** durable process повторяет delivery по bounded policy
- **THEN** authoritative Auction state остаётся неизменным

#### Scenario: Email окончательно отклонён
- **WHEN** Notify исчерпал retry budget и сохранил `FAILED`
- **THEN** outbid process завершается с наблюдаемым delivery outcome без owner-команд в Auction
