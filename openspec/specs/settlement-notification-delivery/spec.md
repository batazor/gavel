# settlement-notification-delivery Specification

## Purpose

Определяет идемпотентную доставку settlement-уведомлений, принадлежащую Notify, с durable историей и безопасным локальным email-каналом.

## Requirements

### Requirement: Notify принимает команды только от trusted Processes workload
Notify SHALL принимать versioned `SendNotification` только от подтверждённой workload identity Processes с разрешённым template и валидным deterministic notification id; браузер и BFF MUST NOT вызывать внутреннюю команду напрямую.

#### Scenario: Processes отправляет разрешённое уведомление
- **WHEN** trusted Processes activity передаёт recipient, versioned template, safe data и notification id
- **THEN** Notify валидирует команду и создаёт либо возвращает authoritative delivery result

#### Scenario: Caller не подтверждён
- **WHEN** команда приходит без trusted Processes identity или требуемого permission
- **THEN** Notify fail closed до сохранения уведомления и обращения к channel adapter

### Requirement: Notification id дедуплицирует повторную доставку
Notify SHALL durable хранить canonical command fingerprint и result по notification id; точный повтор MUST вернуть сохранённый outcome, а повтор id с другим payload MUST завершиться conflict.

#### Scenario: RPC response потерян после commit
- **WHEN** Processes повторяет ту же команду с прежним notification id
- **THEN** Notify возвращает эквивалентный result без второго email

#### Scenario: Notification id переиспользован
- **WHEN** прежний notification id используется с другим recipient, template или data
- **THEN** Notify возвращает typed conflict без изменения сохранённой команды

### Requirement: Delivery lifecycle принадлежит Notify
Notify SHALL хранить состояние `PENDING`, `SENT` либо `FAILED`, число попыток, versioned template reference и безопасную audit history; Processes MUST NOT определять channel-specific outcome.

#### Scenario: Email принят локальным каналом
- **WHEN** Mailpit-compatible adapter подтверждает доставку сообщения
- **THEN** Notify фиксирует `SENT` и возвращает authoritative delivery outcome

#### Scenario: Канал временно недоступен
- **WHEN** adapter возвращает transient failure
- **THEN** Notify выполняет bounded retry с тем же notification id
- **THEN** повтор не создаёт новую логическую notification запись

#### Scenario: Канал отклонил сообщение постоянно
- **WHEN** retry budget исчерпан либо adapter возвращает permanent rejection
- **THEN** Notify фиксирует `FAILED` с безопасной причиной
- **THEN** Auction и Payments state не изменяются

### Requirement: Settlement templates раскрывают только данные получателя
Notify SHALL рендерить versioned templates для recovery, candidate failure и winner confirmation только из переданных владельцем Processes safe data; recovery URL MUST отправляться лишь соответствующему recipient и не попадать в public events или telemetry.

#### Scenario: Кандидату требуется recovery
- **WHEN** Processes после authoritative Payments read отправляет recovery notification
- **THEN** Notify доставляет recovery URL только адресу текущего кандидата
- **THEN** message и telemetry не содержат чужую identity, secret maximum или provider object

#### Scenario: Победитель подтверждён
- **WHEN** Processes после authoritative Auction confirmation отправляет winner notification
- **THEN** Notify использует winner template и стабильный notification id для terminal round

### Requirement: История доставки доступна для локальной проверки
Локальный MVP SHALL предоставлять оператору безопасный способ подтвердить template, recipient, status и число попыток без чтения приватной базы другого bounded context.

#### Scenario: Smoke test проверяет уведомление
- **WHEN** settlement workflow завершил notification activity
- **THEN** проверка видит ровно одну логическую запись с ожидаемым recipient/template и terminal delivery status
- **THEN** проверка не требует доступа к Auction private events или Payments provider data
