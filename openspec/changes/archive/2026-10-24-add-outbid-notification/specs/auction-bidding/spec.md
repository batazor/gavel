# Spec Delta

## ADDED Requirements

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
