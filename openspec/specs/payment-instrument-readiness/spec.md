# payment-instrument-readiness Specification

## Purpose

Определяет безопасную подготовку платёжного инструмента и явного разрешения на будущее списание до участия пользователя в торгах.

## Requirements

### Requirement: Регистрация фиксирует явное разрешение на списание
Payments SHALL начинать регистрацию только для authenticated пользователя с permission `platform:register_payment_instrument`, допустимыми recovery email и terms version и новым idempotency key.

#### Scenario: Пользователь принимает условия
- **WHEN** пользователь отправляет допустимый `BeginInstrumentRegistration`
- **THEN** Payments фиксирует `DebitAuthorization` с terms version и server-controlled accepted-at
- **THEN** response возвращает provider-hosted URL и безопасный статус без карточных данных или provider refs

#### Scenario: Permission отсутствует или Authz недоступен
- **WHEN** permission check возвращает deny, timeout либо dependency failure
- **THEN** Payments fail closed и не создаёт профиль, provider registration или outbox event

### Requirement: Карточные данные не проходят через Gavel
Payments MUST направлять пользователя на provider-hosted registration и MUST NOT принимать, хранить, логировать или публиковать PAN, CVC и иные секретные реквизиты инструмента.

#### Scenario: Пользователь вводит платёжные данные
- **WHEN** пользователь открывает выданный registration URL
- **THEN** данные передаются непосредственно платёжному провайдеру
- **THEN** Web, Cosmo Router и Payments получают только непрозрачный результат регистрации и безопасные display-поля

### Requirement: Готовность подтверждается authoritative исходом провайдера
Payments SHALL переводить инструмент в `READY` только после проверенного provider outcome и действующего `DebitAuthorization`; готовность MUST NOT трактоваться как резерв средств или гарантия списания.

#### Scenario: Провайдер подтвердил инструмент
- **WHEN** проверенный callback соответствует ожидающей регистрации пользователя
- **THEN** Payments атомарно сохраняет opaque refs, безопасные display-поля и состояние `READY`
- **THEN** та же transaction создаёт versioned `PaymentInstrumentReady`

#### Scenario: Провайдер не подтвердил инструмент
- **WHEN** provider outcome отклонён, некорректен или не завершён
- **THEN** инструмент не становится `READY`
- **THEN** публичное состояние не обещает возможность будущего списания

### Requirement: Отзыв запрещает только новые обязательства
Payments SHALL позволять владельцу с permission `payment_profile:manage` отозвать инструмент или разрешение и атомарно публиковать `PaymentInstrumentRevoked`, не изменяя уже принятые Auction обязательства.

#### Scenario: Владелец отзывает готовность
- **WHEN** авторизованный владелец отзывает активный инструмент
- **THEN** Payments переводит профиль в `REVOKED` и публикует asynchronous `PaymentInstrumentRevoked`
- **THEN** Auction как owner eligibility запрещает новые ставки после применения versioned события

#### Scenario: Уже принятая ставка существует
- **WHEN** инструмент отозван после принятой Auction ставки
- **THEN** Payments не отменяет ставку и не изменяет settlement candidates
- **THEN** дальнейшее решение остаётся за Auction и Processes

### Requirement: Readiness events устойчивы к повторной доставке
Payments SHALL публиковать минимальные versioned readiness facts через transactional outbox, а Auction SHALL применять их идемпотентно и монотонно по source version.

#### Scenario: Commit готовности успешен
- **WHEN** Payments фиксирует новый readiness state
- **THEN** outbox event содержит event/correlation ids, user id, aggregate version и нейтральный status
- **THEN** event не содержит recovery email, terms, provider refs или credentials

#### Scenario: Событие доставлено повторно или не по порядку
- **WHEN** Auction получает duplicate либо stale readiness event
- **THEN** его локальная eligibility projection не откатывается и не создаёт второй effect

### Requirement: Публичное чтение readiness сохраняет privacy
Payments SHALL возвращать пользователю только собственный status и безопасные display-поля и MUST NOT раскрывать provider refs, Authz revisions, receipts или чужие payment profiles.

#### Scenario: Пользователь читает свой инструмент
- **WHEN** authenticated пользователь запрашивает `myPaymentInstrument`
- **THEN** Payments возвращает delivery-safe состояние текущего пользователя

#### Scenario: Dependency read недоступен
- **WHEN** authoritative payments-db read завершается timeout или transport failure
- **THEN** клиент получает retryable safe error без различимого private metadata
