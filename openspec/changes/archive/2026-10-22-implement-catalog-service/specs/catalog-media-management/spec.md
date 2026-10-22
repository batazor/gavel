# Spec Delta

## Purpose

Определяет безопасный lifecycle изображений лота, при котором Catalog владеет metadata и связями, а бинарные данные загружаются напрямую в управляемое object storage.

## ADDED Requirements

### Requirement: Продавец получает ограниченное upload intent
Catalog SHALL создавать server-generated image id и object key для допустимого MIME type, размера и SHA-256 checksum и SHALL возвращать короткоживущий presigned PUT только authenticated продавцу.

#### Scenario: Допустимое изображение подготовлено к загрузке
- **WHEN** продавец вызывает `requestLotImageUpload` с поддерживаемым type, bounded length, checksum и новым idempotency key
- **THEN** Catalog сохраняет принадлежащее продавцу состояние `PENDING`
- **THEN** response содержит image id, expiry и обязательные upload headers без storage credentials или внутреннего object key

#### Scenario: Upload input недопустим
- **WHEN** type, size или checksum нарушает Catalog policy
- **THEN** request отклоняется до создания metadata и presigned URL

### Requirement: Бинарные данные не проходят через application data plane
Web SHALL загружать bytes напрямую по presigned PUT, а Cosmo Router и Catalog SHALL обрабатывать только intent, metadata и completion command.

#### Scenario: Клиент загружает объект
- **WHEN** Web использует выданный upload URL и обязательные headers
- **THEN** object storage принимает только предусмотренный method/key/metadata в пределах TTL
- **THEN** image bytes не передаются через GraphQL resolver или Catalog runtime

### Requirement: Завершение загрузки подтверждает целостность
Catalog MUST сверить фактические object size, MIME type и checksum с ожидаемыми значениями до перехода `PENDING → READY`.

#### Scenario: Metadata совпадает
- **WHEN** продавец завершает загрузку и authoritative object metadata совпадает с intent
- **THEN** image становится `READY`
- **THEN** exact replay completion возвращает то же состояние без второго бизнес-эффекта

#### Scenario: Metadata не совпадает
- **WHEN** size, type, checksum либо объект отсутствует
- **THEN** image не становится `READY` и не может быть прикреплено к лоту
- **THEN** объект помечается для безопасного удаления

### Requirement: К лоту прикрепляются только собственные READY изображения
Catalog SHALL атомарно переводить выбранные изображения `READY → ATTACHED` вместе с созданием лота и MUST запретить чужие, повторяющиеся, просроченные или уже прикреплённые image ids.

#### Scenario: Продавец прикрепляет упорядоченные изображения
- **WHEN** `ListLot` содержит от 1 до 20 уникальных READY images текущего продавца
- **THEN** Catalog сохраняет заданный порядок и alt-текст вместе с лотом
- **THEN** каждое image становится `ATTACHED` к одному lot id

#### Scenario: Один image непригоден
- **WHEN** хотя бы один image не принадлежит продавцу или не находится в `READY`
- **THEN** весь `ListLot` отклоняется без лота, attachment и outbox event

### Requirement: Reconciliation удаляет orphan media
Catalog SHALL обнаруживать просроченные `PENDING`, неприкреплённые `READY` и объекты после неуспешной проверки и SHALL идемпотентно удалять object и metadata по lifecycle policy.

#### Scenario: Клиент не завершил upload
- **WHEN** PENDING intent превысил TTL и не прикреплён к лоту
- **THEN** reconciliation удаляет соответствующий storage object, если он существует
- **THEN** повтор reconciliation безопасен и не затрагивает `ATTACHED` images

### Requirement: Media telemetry сохраняет privacy
Catalog SHALL наблюдать upload intents, completion и cleanup через bounded outcomes и MUST NOT помещать seller/image ids, object keys, URLs, checksums или credentials в labels и logs.

#### Scenario: Проверка объекта завершилась ошибкой
- **WHEN** storage dependency недоступна или metadata не совпала
- **THEN** metric различает bounded operation/outcome и retryability
- **THEN** trace/log не содержит presigned URL или storage identifier
