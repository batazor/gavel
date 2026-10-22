# Spec Delta

## Purpose

Определяет безопасное provision/revoke authorization relationships владельцами ресурсов и стабильную регистрацию неперсонального Gavel principal после verified login.

## ADDED Requirements

### Requirement: Verified login создаёт канонический Gavel principal
Authz SHALL детерминированно выводить непрозрачный principal id из verified `(issuer, subject)` и SHALL идемпотентно создавать membership `platform:gavel#member` только по команде доверенного BFF workload.

#### Scenario: Пользователь входит впервые
- **WHEN** BFF после успешной JWT verification вызывает `RegisterPrincipal` с issuer, subject и idempotency key
- **THEN** Authz возвращает стабильный principal id и ZedToken membership write
- **THEN** raw issuer и subject не возвращаются в response, не сохраняются в receipts и не попадают в logs или metric labels

#### Scenario: Login доставлен повторно
- **WHEN** BFF повторяет тот же request после неопределённого transport outcome
- **THEN** Authz возвращает тот же principal id и сохранённый authoritative result
- **THEN** effective membership эквивалентен однократной регистрации

#### Scenario: Другой workload пытается зарегистрировать principal
- **WHEN** `RegisterPrincipal` вызывает workload, которому эта capability не разрешена
- **THEN** Authz возвращает permission-denied без изменения relationship graph

### Requirement: Relationship writer ограничен владельцем ресурса
Authz MUST выводить caller из trusted workload identity и SHALL разрешать mutation только для явно заданных resource types и relations, которыми этот workload владеет.

#### Scenario: Auction provision участника
- **WHEN** workload Auction создаёт разрешённую relation `auction:participant`
- **THEN** Authz принимает mutation и возвращает revision token

#### Scenario: Auction пытается изменить payment relationship
- **WHEN** workload Auction отправляет mutation ресурса `payment_profile` или relation вне своей allowlist
- **THEN** Authz возвращает typed scope rejection до обращения к SpiceDB

#### Scenario: Caller передаёт workload identity в payload
- **WHEN** application payload или произвольный metadata header пытается подменить caller
- **THEN** Authz использует только identity из trusted mesh context
- **THEN** неподтверждённый caller получает unauthenticated outcome

### Requirement: Batch relationship mutation применяется целиком
Authz SHALL применять валидированный bounded batch TOUCH/delete mutations одной authoritative operation и MUST NOT оставлять частично применённый batch при rejection.

#### Scenario: Owner создаёт обязательные grants
- **WHEN** Catalog provision разрешённые `lot:platform` и `lot:seller` relationships одним request
- **THEN** обе связи становятся видимы на returned ZedToken
- **THEN** последующая causal check может требовать revision не старее этого token

#### Scenario: Одна mutation в batch недопустима
- **WHEN** хотя бы один элемент batch нарушает ownership matrix или schema shape
- **THEN** Authz отклоняет весь batch до authoritative write

#### Scenario: Owner отзывает relationship
- **WHEN** разрешённый owner отправляет delete существующей или уже отсутствующей relation
- **THEN** effective relationship отсутствует после returned ZedToken
- **THEN** повтор того же delete безопасен

### Requirement: Relationship command имеет durable idempotency
Каждая registration или relationship mutation MUST использовать idempotency key; Authz SHALL durable фиксировать payload fingerprint до внешнего эффекта и сохранять успешный result для повторной доставки.

#### Scenario: Response потерян после SpiceDB write
- **WHEN** relationship write мог завершиться, но caller не получил response
- **THEN** повтор с тем же key и payload безопасно завершает или воспроизводит команду
- **THEN** effective graph соответствует однократному TOUCH/delete batch

#### Scenario: Key повторно использован с другим payload
- **WHEN** тот же caller передаёт прежний idempotency key с другим principal или набором relationships
- **THEN** Authz возвращает typed idempotency conflict
- **THEN** второй payload не изменяет relationship graph

#### Scenario: Незавершённая команда восстанавливается
- **WHEN** instance прекращает работу после durable claim, но до сохранения результата
- **THEN** другой instance может безопасно продолжить тот же fingerprint после bounded lease
- **THEN** несовместимый fingerprint не может захватить command key

### Requirement: Dependency failure не маскируется как успешный write
Authz SHALL возвращать retryable failure при недоступности SpiceDB или receipt store и MUST NOT подтверждать command без authoritative revision token и durable result.

#### Scenario: Receipt store недоступен до write
- **WHEN** Authz не может durable claim idempotency key
- **THEN** SpiceDB mutation не выполняется
- **THEN** caller получает retryable unavailable status

#### Scenario: SpiceDB недоступен после claim
- **WHEN** authoritative mutation завершается timeout или transport failure
- **THEN** command остаётся безопасным для retry с тем же fingerprint
- **THEN** Authz не возвращает `written` outcome

### Requirement: Provisioning telemetry сохраняет privacy
Authz SHALL наблюдать registration, write, delete, replay, conflict и recovery через bounded labels и MUST NOT логировать issuer, subject, relationship ids, idempotency keys, ZedTokens или credentials.

#### Scenario: Идемпотентный replay наблюдаем
- **WHEN** command возвращается из durable receipt
- **THEN** счётчик replay увеличивается без caller/resource identifiers
- **THEN** structured log содержит только operation, bounded outcome и correlation context
