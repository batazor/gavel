# local-development-runtime Specification

## Purpose

Определяет воспроизводимое локальное окружение, в котором разработчик одной командой запускает и проверяет полный минимальный аукционный путь без внешних платных сервисов.

## Requirements

### Requirement: MVP runtime управляется одной точкой входа
Репозиторий SHALL предоставлять документированные команды запуска и остановки локального `kind` runtime; повторный запуск MUST сходиться к тому же готовому состоянию без ручной настройки отдельных сервисов.

#### Scenario: Чистый локальный запуск
- **WHEN** разработчик с установленными prerequisite запускает MVP up command из корня репозитория
- **THEN** создаётся либо переиспользуется выделенный cluster и устанавливаются обязательные platform и service workloads
- **THEN** команда завершается успешно только после readiness обязательного пользовательского пути

#### Scenario: Повторный запуск
- **WHEN** MVP up command выполняется для уже существующего готового runtime
- **THEN** configuration и migrations применяются идемпотентно без потери committed demo state

#### Scenario: Остановка окружения
- **WHEN** разработчик запускает MVP down command
- **THEN** удаляется только выделенное Gavel local окружение, не затрагивая другие clusters или пользовательские данные вне его границы

### Requirement: Runtime содержит только обязательные MVP dependencies
Локальный runtime SHALL запускать BFF/Web, Catalog, Auction, Payments, Processes, Notify, Authz, service-owned datastores, Temporal, Redpanda, outbox connector и локальный email sink; live payment, public edge и внешние credentials MUST NOT требоваться.

#### Scenario: Внешняя сеть недоступна после подготовки images
- **WHEN** обязательные images и dependencies уже доступны локально
- **THEN** аукционный smoke path выполняется без Stripe, OAuth SaaS, Cloudflare или внешнего email provider

#### Scenario: Необязательный компонент отсутствует
- **WHEN** Analytics, realtime subscription adapter или production observability stack не установлен
- **THEN** MVP readiness и основной аукционный путь остаются работоспособными

### Requirement: Seed создаёт детерминированных actors и данные
Bootstrap SHALL идемпотентно подготовить продавца, двух участников, trusted workload identities, Authz relationships, payment readiness и один опубликованный короткий аукцион без хранения production credentials.

#### Scenario: Seed выполняется впервые
- **WHEN** пустой runtime достигает database и Authz readiness
- **THEN** создаются известные local-only identities, разрешения, payment instruments и опубликованный lot/auction
- **THEN** значения, необходимые разработчику для входа и smoke, выводятся безопасно и документируются

#### Scenario: Seed выполняется повторно
- **WHEN** bootstrap повторяется после частичного либо полного успеха
- **THEN** он возвращает те же логические fixtures или эквивалентное готовое состояние без дублей

### Requirement: Readiness проверяет пользовательский путь, а не только процессы
MVP SHALL считаться готовым только когда публичный GraphQL endpoint отвечает owner-backed данными, event delivery может сигнализировать Temporal, внутренние owner RPC доступны, а Notify может доставить сообщение в локальный sink.

#### Scenario: Контейнеры запущены, но connector не готов
- **WHEN** service health endpoints успешны, но outbox delivery path не способен передать контрольное событие
- **THEN** MVP readiness остаётся неуспешным и сообщает failing dependency

#### Scenario: Все обязательные границы готовы
- **WHEN** queries, broker route, Temporal, owner RPC и email sink проходят bounded probes
- **THEN** runtime объявляет готовность к smoke/E2E сценарию

#### Scenario: Owner события подготовили Auction к участию
- **WHEN** seed публикует lot через Catalog и подготавливает payment instruments через Payments
- **THEN** Catalog `LotListed` открывает соответствующий Auction через broker delivery
- **THEN** Payments readiness events обновляют authoritative Auction projection допуска без прямой записи в Auction storage

### Requirement: Финальная проверка подтверждает три сквозных исхода
Репозиторий SHALL предоставлять один repeatable MVP verification target, который после сборки окружения проверяет happy settlement, переход к следующему кандидату и восстановление после перезапуска Processes.

#### Scenario: Первый кандидат успешно оплачивает
- **WHEN** два участника сделали ставки и sandbox payment текущего кандидата завершается успехом
- **THEN** Auction достигает `SOLD`, Payments содержит один success, Web polling видит terminal result, а Notify содержит одно terminal уведомление

#### Scenario: Первый кандидат не оплачивает
- **WHEN** sandbox outcome первого settlement round становится authoritative `EXPIRED`
- **THEN** Auction выбирает следующий round и только следующий успешный candidate становится winner
- **THEN** на auction существует не более одного успешного payment

#### Scenario: Processes перезапущен во время ожидания
- **WHEN** worker перезапускается после приёма event до завершения settlement
- **THEN** workflow продолжается из durable history и достигает terminal outcome без повторных owner effects или notification delivery

### Requirement: Failure оставляет пригодную диагностику
При неуспешном запуске или smoke runtime SHALL сохранить bounded logs, workload status, connector lag, Temporal workflow identity и quarantine summary, не раскрывая secrets или private event payload.

#### Scenario: Финальная проверка не прошла
- **WHEN** любой acceptance scenario превышает timeout или получает неожиданный outcome
- **THEN** команда завершается non-zero и указывает failing boundary
- **THEN** диагностический bundle достаточен для воспроизведения без включения credentials, recovery URL или secret bid maximum
