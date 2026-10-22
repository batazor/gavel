# Spec Delta

## ADDED Requirements

### Requirement: Catalog WithdrawLot является владельческой командой saga
Catalog SHALL владеть additive `gavel.catalog.v1.WithdrawLot`, принимать её только от trusted Processes workload с deadline и idempotency key и возвращать authoritative success либо stable typed rejection.

#### Scenario: Processes завершает успешную saga
- **WHEN** Processes после принятого `CancelAuction` синхронно вызывает `WithdrawLot` с тем же operation intent
- **THEN** Catalog выполняет permission check `lot:execute_withdraw`, фиксирует `WITHDRAWN` и возвращает authoritative lot version
- **THEN** `LotWithdrawn` публикуется отдельно через Catalog outbox как asynchronous факт

#### Scenario: Неподтверждённый workload вызывает WithdrawLot
- **WHEN** caller не имеет trusted mesh identity Processes либо пытается подменить её metadata/payload
- **THEN** Catalog возвращает unauthenticated/permission-denied до domain mutation

#### Scenario: Processes повторяет command после timeout
- **WHEN** exact `WithdrawLot` повторён с тем же idempotency key
- **THEN** Catalog возвращает сохранённую authoritative version без второго state transition или outbox event
- **THEN** тот же key с другим payload отклоняется как typed conflict
