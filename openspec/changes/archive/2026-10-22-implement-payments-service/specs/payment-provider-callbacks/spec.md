# Spec Delta

## Purpose

Определяет безопасную обработку внешних payment provider callbacks с проверкой подлинности, дедупликацией и переводом в нейтральные состояния Payments.

## ADDED Requirements

### Requirement: Callback использует отдельную публичную границу
Provider callback SHALL поступать напрямую в Payments по выделенному Cloudflare/Istio route без browser JWT, cache, transformation или challenge и MUST NOT проходить через Cosmo Router.

#### Scenario: Провайдер отправляет уведомление
- **WHEN** request приходит на зарегистрированный provider callback path
- **THEN** edge передаёт неизменённые body и signature headers в Payments
- **THEN** GraphQL resolvers и browser authentication не участвуют в обработке

### Requirement: Подпись проверяется до доверия payload
Payments MUST выбирать adapter по зарегистрированному callback path и проверять подпись, timestamp tolerance и configured secret до отображения payload в domain outcome.

#### Scenario: Подпись корректна
- **WHEN** adapter подтверждает signature и freshness исходного body
- **THEN** callback может быть преобразован в provider-neutral event reference и outcome

#### Scenario: Подпись отсутствует, просрочена или неверна
- **WHEN** verification не проходит
- **THEN** Payments отклоняет request без database mutation, provider acknowledgement успеха или раскрытия причины злоумышленнику

### Requirement: Provider event обрабатывается ровно один раз логически
Payments SHALL durable дедуплицировать проверенный callback по `(provider, provider_event_ref)` и сохранять fingerprint, чтобы exact duplicate был успешным replay, а incompatible reuse отклонялся.

#### Scenario: Callback доставлен повторно
- **WHEN** provider повторяет тот же event reference и canonical payload
- **THEN** Payments возвращает сохранённый acknowledgement без второго transition или outbox event

#### Scenario: Event reference использован для другого payload
- **WHEN** verified callback имеет существующий event reference, но другой canonical fingerprint
- **THEN** Payments фиксирует security-safe conflict и не изменяет aggregate

### Requirement: Callback применяется только к ожидаемому aggregate
Payments SHALL сопоставлять opaque provider refs с owned instrument или attempt и SHALL применять только разрешённый текущим состоянием монотонный transition.

#### Scenario: Поздний callback соответствует terminal state
- **WHEN** callback повторяет либо предшествует уже зафиксированному terminal outcome
- **THEN** Payments сохраняет текущий state и не публикует противоречащий event

#### Scenario: Provider ref неизвестен
- **WHEN** проверенный callback невозможно сопоставить owned aggregate
- **THEN** Payments не создаёт профиль или attempt из внешнего payload
- **THEN** callback сохраняется как bounded reconciliation/security outcome без утечки ref

### Requirement: Callback state и outbox фиксируются атомарно
Payments SHALL одной local transaction сохранить inbox receipt, aggregate transition, command/result metadata и соответствующий versioned outbox event.

#### Scenario: Transaction committed
- **WHEN** callback допустимо изменяет instrument или attempt
- **THEN** последующее authoritative read видит новый state и ровно один связанный event готов к доставке

#### Scenario: Transaction откатилась
- **WHEN** database commit не состоялся
- **THEN** provider retry может безопасно повторить полный effect
- **THEN** ни dedup receipt, ни partial state, ни outbox event не остаются подтверждёнными отдельно

### Requirement: Callback acknowledgement поддерживает безопасные повторы
Payments SHALL подтверждать provider request только после durable обработки или exact replay и SHALL возвращать retryable response при временной внутренней ошибке.

#### Scenario: Database временно недоступна
- **WHEN** подпись корректна, но callback нельзя durable применить или дедуплицировать
- **THEN** Payments не возвращает успешный acknowledgement и ожидает повтор провайдера

#### Scenario: Domain outcome успешно сохранён
- **WHEN** transaction committed либо callback является exact duplicate
- **THEN** Payments возвращает provider-compatible success без внутренних identifiers

### Requirement: Callback telemetry сохраняет секреты
Payments SHALL наблюдать provider kind, bounded verification/processing outcome и latency и MUST NOT логировать raw body, signature, event ref, customer/instrument refs или payment data.

#### Scenario: Verification завершается ошибкой
- **WHEN** callback отклонён до domain mapping
- **THEN** security metric увеличивается с bounded provider/outcome labels
- **THEN** trace и structured log не содержат secret headers или raw payload
