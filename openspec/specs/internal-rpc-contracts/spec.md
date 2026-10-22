# internal-rpc-contracts Specification

## Purpose

Определяет безопасные и совместимые синхронные межсервисные команды Gavel, не смешивая их с публичным GraphQL API и асинхронными доменными событиями.

## Requirements

### Requirement: Синхронные внутренние вызовы имеют явную границу
Система SHALL использовать versioned RPC contract для синхронной команды или чтения между bounded contexts, а браузер SHALL обращаться только к публичной GraphQL Federation границе.

#### Scenario: Temporal activity вызывает владельца команды
- **WHEN** Processes выполняет activity закрытия аукциона, снятия лота, оплаты или уведомления
- **THEN** activity вызывает versioned RPC соответствующего владельца Auction, Catalog, Payments или Notify
- **THEN** владелец принимает окончательное доменное решение и возвращает типизированный результат

#### Scenario: Публичный запрос проходит через BFF
- **WHEN** браузер выполняет query, mutation или subscription
- **THEN** запрос проходит через Cosmo Router и владельческий Federation subgraph по GraphQL/HTTP
- **THEN** браузер не получает доступ к внутреннему RPC endpoint

### Requirement: Команды и факты используют разные контракты
Система MUST отделять синхронный RPC command contract от асинхронного event contract, даже если оба сериализованы Protobuf.

#### Scenario: Принятая RPC-команда создаёт факт
- **WHEN** владелец контекста принимает RPC-команду и фиксирует изменение состояния
- **THEN** он атомарно создаёт свой outbox event
- **THEN** подписчики получают факт через Redpanda, а не через повторное использование RPC response как события

#### Scenario: Событие запускает процесс
- **WHEN** Processes получает доменный факт из Redpanda
- **THEN** он запускает или сигнализирует Temporal workflow асинхронно
- **THEN** последующие намерения workflow выполняются владельческими RPC-командами

### Requirement: Владелец сервиса владеет RPC contract
Каждый RPC contract MUST принадлежать bounded context, который исполняет операцию; потребители SHALL зависеть от versioned контракта владельца и не определять копии его сообщений.

#### Scenario: Processes вызывает Payments
- **WHEN** Processes генерирует клиент для попытки оплаты
- **THEN** источником contract является module Payments
- **THEN** Processes не публикует собственную копию payment request или provider-specific объект

#### Scenario: Независимая эволюция контракта
- **WHEN** владелец добавляет совместимое поле в пакет `v1`
- **THEN** старые потребители продолжают работать
- **THEN** несовместимое изменение публикуется в новом versioned package

### Requirement: Повтор команды безопасен
Каждая изменяющая RPC-команда SHALL принимать стабильный idempotency key, а владелец SHALL возвращать ранее зафиксированный результат для повторной доставки того же намерения.

#### Scenario: Temporal повторяет activity после timeout
- **WHEN** первый вызов мог быть зафиксирован, но Processes не получил response
- **THEN** повтор передаёт тот же idempotency key
- **THEN** владелец не выполняет бизнес-эффект второй раз и возвращает эквивалентный authoritative result

#### Scenario: Ключ повторно использован с другим payload
- **WHEN** caller отправляет тот же idempotency key с несовместимым payload
- **THEN** владелец отклоняет запрос как конфликт, не изменяя состояние

### Requirement: RPC имеет ограниченное время выполнения и типизированные отказы
Каждый caller MUST передавать deadline, а владелец SHALL различать retryable transport failure, ожидаемый domain rejection и успешный результат.

#### Scenario: Deadline исчерпан
- **WHEN** RPC не завершился до caller deadline
- **THEN** caller получает retryable deadline failure
- **THEN** mutation не считается незафиксированной без проверки по idempotency key

#### Scenario: Домен отклоняет команду
- **WHEN** команда нарушает текущее состояние или бизнес-инвариант владельца
- **THEN** владелец возвращает стабильный typed domain error
- **THEN** Temporal workflow обрабатывает его как ожидаемый исход, а не как бесконечно retryable transport failure

### Requirement: Внутренний RPC авторизуется на стороне владельца
Владелец команды MUST аутентифицировать workload identity вызывающего сервиса и выполнить authoritative permission check; переданный в payload произвольный principal не SHALL считаться доверенным.

#### Scenario: Processes выполняет системную команду
- **WHEN** Processes вызывает Auction, Catalog, Payments или Notify
- **THEN** сервис выводит `service_account:processes` из mTLS workload identity
- **THEN** сервис проверяет соответствующее permission через Authz до доменного изменения

#### Scenario: Недоступен Authz
- **WHEN** permission check для mutation завершается timeout или ошибкой
- **THEN** владелец fail closed и возвращает retryable failure без выполнения команды

### Requirement: Совместимость contract проверяется автоматически
Repository SHALL проверять формат, lint, сборку и breaking changes Protobuf contracts до их принятия.

#### Scenario: Совместимое изменение
- **WHEN** change добавляет поле или RPC без нарушения принятой compatibility policy
- **THEN** Buf checks проходят и generated clients могут быть обновлены независимо

#### Scenario: Несовместимое изменение v1
- **WHEN** change удаляет поле, меняет его wire type или нарушает service contract пакета `v1`
- **THEN** CI отклоняет change до merge

### Requirement: RPC наблюдаем как отдельная зависимость
Caller и owner SHALL публиковать latency, request rate и failure metrics по service/method/status без entity identifiers, а distributed trace SHALL переносить correlation context через RPC boundary.

#### Scenario: RPC деградирует
- **WHEN** P99 latency или retryable failure ratio метода превышает локальный порог
- **THEN** dashboard владельца и потребителя позволяют отличить transport failure от domain rejection
- **THEN** trace связывает Temporal activity либо входной subgraph resolver с владельческой операцией

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

### Requirement: Payments settlement commands принадлежат Payments
Payments SHALL владеть additive `gavel.payments.v1.AttemptPayment` и `ExpirePayment`, принимать их только от trusted Processes workload с deadline и idempotency key и возвращать provider-neutral authoritative outcome.

#### Scenario: Processes начинает попытку оплаты
- **WHEN** Processes вызывает `AttemptPayment` с authoritative auction, round, candidate и amount
- **THEN** Payments проверяет permission `payment_attempt:process` и возвращает stable attempt id/status либо typed domain rejection
- **THEN** дальнейшие lifecycle facts публикуются отдельно через Payments outbox

#### Scenario: Processes завершает recovery window
- **WHEN** Processes вызывает `ExpirePayment` для существующей попытки
- **THEN** Payments подтверждает отсутствие success и безопасный cancel до authoritative `EXPIRED`
- **THEN** неопределённый provider outcome возвращается как retryable failure, а не ложный expiry

#### Scenario: Неподтверждённый workload вызывает payment command
- **WHEN** caller не имеет trusted mesh identity Processes либо пытается передать identity в metadata/payload
- **THEN** Payments возвращает unauthenticated/permission-denied до provider или domain effect

#### Scenario: Processes повторяет command после timeout
- **WHEN** exact payment command повторён с тем же idempotency key
- **THEN** Payments возвращает сохранённый authoritative result без второго provider call, transition или outbox event
- **THEN** тот же key с другим payload отклоняется как typed conflict

### Requirement: Payments RPC распространяется как versioned Generated SDK
Payments MUST публиковать owner module `buf.build/gavel/payments` в BSR, а Rust consumers SHALL использовать Generated Cargo SDK с точной версией module commit и plugin вместо локальной компиляции или копирования Payments `.proto`.

#### Scenario: Owner публикует совместимый contract
- **WHEN** Payments меняет RPC schema additive и change проходит lint/build/breaking checks
- **THEN** schema публикуется в BSR под change label до обновления consumers
- **THEN** BSR создаёт Generated Prost/Tonic SDK для того же module commit

#### Scenario: Processes обновляет Payments client
- **WHEN** Processes принимает новую совместимую версию owner contract
- **THEN** его Cargo dependency закрепляет точную BSR SDK version, разрешённую через registry metadata и `Cargo.lock`
- **THEN** Processes не компилирует и не хранит собственную копию Payments schema или generated stubs

#### Scenario: Registry package не соответствует проверенному commit
- **WHEN** SDK version невозможно разрешить к ожидаемому Payments module commit либо package недоступен
- **THEN** consumer build завершается ошибкой вместо fallback на локально сгенерированный или плавающий contract
