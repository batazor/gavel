# Spec Delta

## Purpose

Определяет минимальный публичный путь участника от просмотра опубликованного лота до ставки и наблюдения итогового статуса аукциона через единую GraphQL-границу.

## ADDED Requirements

### Requirement: Workspace доступен через единую публичную границу
BFF SHALL предоставлять один GraphQL workspace, который компонует delivery-safe карточку владельца Catalog и публичное состояние владельца Auction; браузер MUST NOT обращаться к context services или внутренним gRPC endpoints напрямую.

#### Scenario: Пользователь открывает опубликованный лот
- **WHEN** браузер запрашивает workspace по lot id через публичный GraphQL endpoint
- **THEN** BFF возвращает карточку Catalog, Auction status, `endsAt`, текущую цену, currency и stream version
- **THEN** ответ не содержит seller identity, секретные максимумы, settlement candidate identity, provider objects или storage keys

#### Scenario: Один владелец временно недоступен
- **WHEN** обязательный owner subgraph не отвечает в пределах bounded timeout
- **THEN** BFF возвращает явную безопасную GraphQL ошибку без выдуманного доменного состояния
- **THEN** повтор запроса не создаёт бизнес-эффектов

### Requirement: Ставка проходит через authoritative Auction mutation
Аутентифицированный участник SHALL размещать ставку через BFF mutation с client-generated idempotency key; BFF MUST передать verified principal владельцу Auction без принятия доменного решения.

#### Scenario: Допустимая ставка принята
- **WHEN** участник с permission и готовым payment instrument отправляет допустимый maximum
- **THEN** Auction authoritative принимает ставку и BFF возвращает публичную цену, accepted version и безопасный viewer outcome
- **THEN** UI обновляет workspace без раскрытия maximum или личности другого участника

#### Scenario: Результат mutation потерян
- **WHEN** клиент повторяет тот же intent с прежним idempotency key после неопределённого transport outcome
- **THEN** Auction возвращает эквивалентный сохранённый результат без второй ставки

#### Scenario: Ставка отклонена
- **WHEN** Auction возвращает forbidden, payment method required, closed auction или idempotency conflict
- **THEN** BFF сохраняет typed safe outcome, а UI показывает различимое действие для пользователя без изменения локального состояния как будто ставка принята

### Requirement: Polling сохраняет монотонное публичное представление
Web SHALL периодически обновлять workspace и MUST не заменять уже применённое Auction состояние ответом с меньшей stream version; terminal `SOLD`, `UNSOLD` или `CANCELLED` SHALL оставаться terminal в пользовательском представлении.

#### Scenario: Poll вернул более новую версию
- **WHEN** очередной workspace query содержит stream version выше локальной
- **THEN** Web применяет новую цену, дедлайн, viewer state и status атомарно для этого аукциона

#### Scenario: Запоздалый poll вернул старую версию
- **WHEN** более медленный запрос возвращает stream version ниже уже отображённой
- **THEN** Web игнорирует устаревшее Auction состояние

#### Scenario: Settlement завершён
- **WHEN** authoritative Auction status становится `SOLD` либо `UNSOLD`
- **THEN** workspace показывает terminal outcome и прекращает предлагать новую ставку
- **THEN** winner identity раскрывается только в разрешённом viewer-specific поле

### Requirement: Demo fallback явно отделён от подключённого MVP
Web MUST явно различать локальные demo data и данные работающего BFF; успешный MVP smoke SHALL выполняться только при настроенном публичном GraphQL endpoint.

#### Scenario: GraphQL endpoint не настроен
- **WHEN** Web запускается без endpoint для BFF
- **THEN** интерфейс маркирует данные как demo и не сообщает о доступности реальных ставок

#### Scenario: GraphQL endpoint настроен
- **WHEN** Web подключён к локальному BFF
- **THEN** workspace использует owner-backed queries и mutations, а transport failure показывается как ошибка подключения, не как demo success
