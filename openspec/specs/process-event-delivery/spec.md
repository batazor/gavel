# process-event-delivery Specification

## Purpose

Определяет надёжную и приватную доставку owner events из committed transactional outbox в durable workflows Processes с безопасным повтором после сбоев.

## Requirements

### Requirement: Только committed owner events поступают в process ingress
Auction и Payments SHALL публиковать process-driving события из собственных transactional outbox после commit; доставка SHALL быть at-least-once и MUST сохранять event id, aggregate id, owner version, schema version и occurred-at metadata.

#### Scenario: Owner transaction зафиксирована
- **WHEN** Auction или Payments атомарно сохраняет доменное состояние и outbox record
- **THEN** event становится доступен асинхронному delivery path и может быть передан Processes

#### Scenario: Owner transaction откатилась
- **WHEN** доменная transaction не была committed
- **THEN** соответствующий process event не публикуется и workflow не сигнализируется

#### Scenario: Событие доставлено повторно
- **WHEN** delivery path повторно передаёт тот же event после restart или неопределённого acknowledgement
- **THEN** Processes получает неизменные identity и version metadata, достаточные для идемпотентного signal-with-start

### Requirement: Restricted settlement events изолированы от публичного потока
Restricted Auction events с candidate identity, round и settlement amount SHALL быть доступны только workload Processes; public consumers MUST NOT получить restricted payload, secret bid maximum или private storage identifiers.

#### Scenario: Выбран settlement candidate
- **WHEN** Auction публикует `SettlementCandidateSelected`
- **THEN** restricted route доставляет событие только авторизованному Processes consumer
- **THEN** публичный Auction topic не содержит candidate identity или secret maximum

#### Scenario: Неавторизованный consumer запрашивает restricted stream
- **WHEN** workload без разрешения пытается читать restricted Auction events
- **THEN** broker authorization отклоняет доступ без раскрытия payload

### Requirement: Acknowledgement следует за durable приёмом workflow
Process ingress SHALL подтверждать broker record только после успешного durable signal-with-start либо доказанного duplicate/stale outcome; transient отказ Temporal MUST приводить к bounded retry без потери события.

#### Scenario: Temporal принял сигнал
- **WHEN** gateway durable запускает или сигнализирует workflow с ожидаемой identity
- **THEN** consumer фиксирует offset и не требует повторной бизнес-обработки этого record

#### Scenario: Temporal временно недоступен
- **WHEN** signal-with-start завершается retryable transport failure
- **THEN** offset не продвигается мимо неподтверждённого record
- **THEN** consumer повторяет доставку с bounded backoff после восстановления

#### Scenario: Consumer перезапущен
- **WHEN** Processes останавливается после чтения record, но до подтверждённого offset
- **THEN** после запуска record читается повторно и сходится к одному workflow effect

### Requirement: Некорректное событие не блокирует поток бесконечно
Ingress SHALL ограниченно повторять только transient failures; несовместимый schema version, невалидный envelope или постоянный routing failure MUST перейти в наблюдаемую quarantine/DLQ запись без исполнения owner commands.

#### Scenario: Envelope невозможно валидировать
- **WHEN** record не содержит обязательную identity/version metadata либо нарушает зарегистрированный contract
- **THEN** Processes не сигнализирует workflow и помещает безопасную диагностическую запись в quarantine
- **THEN** consumer может продолжить обработку последующих допустимых records по определённой policy

#### Scenario: Quarantine record переигрывается после исправления
- **WHEN** оператор устраняет contract/configuration cause и явно запускает replay
- **THEN** исходное событие повторно проходит ту же validation и idempotency границу

### Requirement: Delivery наблюдаема без приватных данных
Delivery path SHALL публиковать bounded-cardinality метрики по owner, event type, route и outcome, а logs/traces MUST NOT помещать candidate identity, amount, secret maximum, payment provider object или payload в labels.

#### Scenario: Возник backlog
- **WHEN** consumer lag превышает локальный MVP threshold
- **THEN** оператор видит lag и failing dependency по bounded labels
- **THEN** telemetry не раскрывает содержимое restricted events
