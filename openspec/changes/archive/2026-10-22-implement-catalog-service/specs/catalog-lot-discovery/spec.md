# Spec Delta

## Purpose

Определяет публичное чтение карточек и bounded cursor-пагинацию Catalog без зависимости от торгового состояния и без раскрытия внутренних storage данных.

## ADDED Requirements

### Requirement: Публичная карточка содержит только delivery-safe поля
Catalog SHALL возвращать опубликованный lot id, title, description и ordered images с delivery URLs/alt, не раскрывая object keys, seller identity, command receipts, Authz revisions или storage credentials.

#### Scenario: Публичный лот найден
- **WHEN** клиент запрашивает существующий `LISTED` lot по id через Federation
- **THEN** Catalog возвращает delivery-safe `Lot`
- **THEN** каждый image URL сформирован delivery adapter из `ATTACHED` metadata

#### Scenario: Лот отсутствует или скрыт
- **WHEN** lot не существует либо не должен быть публичным в текущем состоянии
- **THEN** `lot(id)` возвращает `null` без различимого internal failure или private metadata

### Requirement: Список лотов использует bounded cursor pagination
Catalog SHALL возвращать детерминированно упорядоченную страницу опубликованных лотов с opaque cursor, bounded `first` и корректным `hasNextPage`.

#### Scenario: Клиент читает первую страницу
- **WHEN** `lots(first, after)` использует допустимый limit без cursor
- **THEN** Catalog возвращает не более `first` edges в стабильном порядке и end cursor последнего элемента

#### Scenario: Cursor malformed или limit превышен
- **WHEN** cursor невозможно проверить либо `first` вне допустимого диапазона
- **THEN** Catalog возвращает safe invalid-argument error до database query

### Requirement: Read model согласован с локальным commit
Catalog SHALL строить публичное чтение из catalog-db состояния, зафиксированного в той же transaction, что соответствующий lifecycle result и outbox event.

#### Scenario: ListLot успешно завершён
- **WHEN** create transaction committed
- **THEN** последующее authoritative Catalog read видит lot и ordered images
- **THEN** отсутствие доставки события в Auction не откатывает локальную карточку

#### Scenario: Transaction откатилась
- **WHEN** create/update transaction не зафиксирована
- **THEN** partial lot или image attachment не появляется в query results

### Requirement: Discovery telemetry имеет bounded cardinality
Catalog SHALL наблюдать single/list reads, latency, page-size buckets и dependency outcomes без lot ids, cursors, search text или image URLs в labels.

#### Scenario: Database read недоступен
- **WHEN** catalog-db query завершается timeout или transport failure
- **THEN** клиент получает retryable safe error
- **THEN** metric показывает bounded read/dependency outcome без request identifiers
