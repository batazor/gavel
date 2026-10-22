# authz-permission-decisions Specification

## Purpose

Определяет authoritative и причинно-согласованные решения о доступе к ресурсам Gavel без переноса доменных инвариантов в authorization graph.

## Requirements

### Requirement: Владелец ресурса получает authoritative permission decision
Authz SHALL проверять permission по каноническим resource, permission и principal references и SHALL возвращать только `ALLOWED` или `DENIED` вместе с revision token проверки.

#### Scenario: Permission разрешён
- **WHEN** owner service синхронно вызывает `CheckPermission` для существующей связи и вычисляемого permission
- **THEN** Authz возвращает `ALLOWED` и непрозрачный checked-at token
- **THEN** owner service отдельно проверяет собственные domain eligibility и invariants

#### Scenario: Permission запрещён
- **WHEN** authorization graph не даёт principal запрошенный permission
- **THEN** Authz возвращает `DENIED`, не раскрывая relationships или причину внутреннего вычисления

### Requirement: Causal permission check учитывает переданный token
Authz SHALL трактовать непустой consistency token как требование проверить permission на revision не старее переданной и MUST NOT заменять его потенциально устаревшим cached allow.

#### Scenario: Проверка следует за relationship write
- **WHEN** owner передаёт ZedToken, полученный после создания relationship
- **THEN** решение вычисляется на revision не старее этого write
- **THEN** успешный ответ содержит token фактически использованной проверки

#### Scenario: Token некорректен
- **WHEN** caller передаёт malformed или неизвестный consistency token
- **THEN** Authz возвращает стабильный invalid-argument outcome без предположения allow

### Requirement: Bulk check сохраняет соответствие входам
Authz SHALL проверять bounded список permissions одним bulk request и SHALL возвращать ровно одно решение на каждый вход в исходном порядке.

#### Scenario: Viewer capabilities проверяются пакетом
- **WHEN** BFF или owner service передаёт допустимый список разных permission checks
- **THEN** Authz возвращает решения в том же порядке и общий checked-at token
- **THEN** deny одного элемента не превращает остальные решения в allow или transport error

#### Scenario: Batch превышает лимит
- **WHEN** число checks превышает документированный runtime limit
- **THEN** Authz отклоняет весь request стабильным resource-exhausted outcome до обращения к authorization graph

### Requirement: Некорректные references не достигают policy engine
Authz MUST отклонять пустые, неизвестные или синтаксически недопустимые resource types, identifiers, permissions и principal types до вычисления permission.

#### Scenario: Caller передаёт неизвестный тип ресурса
- **WHEN** request использует resource или principal type вне schema contract Gavel
- **THEN** Authz возвращает invalid-argument outcome
- **THEN** SpiceDB request не выполняется

#### Scenario: Request содержит пустой identifier
- **WHEN** resource id, principal id или permission отсутствует
- **THEN** весь single или bulk request отклоняется без частичных решений

### Requirement: Недоступность policy engine обрабатывается fail closed
Authz MUST NOT возвращать cached или предполагаемый allow при timeout, transport failure или invalid response SpiceDB и SHALL обозначать dependency failure как retryable.

#### Scenario: SpiceDB превысил deadline
- **WHEN** authoritative check не завершился в локальный bounded deadline
- **THEN** caller получает retryable deadline/unavailable status вместо permission decision

#### Scenario: SpiceDB вернул неизвестный outcome
- **WHEN** dependency response невозможно безопасно сопоставить `ALLOWED` или `DENIED`
- **THEN** Authz возвращает internal/dependency failure и не разрешает операцию

### Requirement: Permission telemetry не раскрывает идентификаторы
Authz SHALL публиковать latency, decision и failure metrics только с bounded operation/outcome labels и MUST NOT помещать resource ids, principal ids, tokens или credentials в logs и metric labels.

#### Scenario: Permission check наблюдаем
- **WHEN** single или bulk check завершается allow, deny либо dependency failure
- **THEN** метрики позволяют отличить operation и bounded outcome
- **THEN** trace correlation не содержит raw authorization inputs в атрибутах низкой кардинальности
