# Spec Delta

## Purpose

Определяет быстрое и безопасное чтение публичной текущей цены с монотонной версией, восстанавливаемым cache и authoritative fallback.

## ADDED Requirements

### Requirement: Публичная цена не раскрывает секреты торгов
Auction SHALL возвращать через `currentPrice` только вычисленные amount, currency и stream version; maximum участников, `user_id` и private leader identity MUST NOT присутствовать в read model или cache.

#### Scenario: Зритель запрашивает цену
- **WHEN** BFF вызывает `currentPrice(auctionId)` для существующего аукциона
- **THEN** Auction возвращает публичную цену и версию snapshot
- **THEN** ответ одинаково безопасен для анонимного и аутентифицированного зрителя

#### Scenario: Аукцион не существует
- **WHEN** запрошенный auction id отсутствует в authoritative projection
- **THEN** Auction возвращает стабильный not-found outcome без создания cache entry

### Requirement: Чтение использует version-aware cache chain
Auction SHALL читать публичную цену через bounded L1, затем Redis L2 и затем authoritative PostgreSQL projection; значение с меньшей версией MUST NOT заменять значение с большей версией.

#### Scenario: L1 snapshot свежий
- **WHEN** L1 содержит snapshot в пределах freshness window
- **THEN** Auction возвращает его без обращения к Redis или PostgreSQL

#### Scenario: Redis содержит неустаревшую версию
- **WHEN** L1 отсутствует или устарел, а Redis возвращает версию не меньше локально увиденной
- **THEN** Auction обновляет L1 и возвращает Redis snapshot

#### Scenario: Fallback новее cache
- **WHEN** Redis отсутствует, недоступен или возвращает устаревшую версию
- **THEN** Auction читает authoritative projection и возвращает её snapshot
- **THEN** opportunistic cache fill применяется только compare-by-version

### Requirement: Cache failure не ломает authoritative чтение
Недоступность Redis или ошибка заполнения cache MUST NOT превращать успешное чтение PostgreSQL projection в пользовательскую ошибку; ограниченные timeout и circuit breaker SHALL предотвращать cascade failure.

#### Scenario: Redis timeout
- **WHEN** Redis не отвечает в пределах локального timeout
- **THEN** Auction продолжает запрос через authoritative projection
- **THEN** результат помечается метриками fallback без раскрытия entity identifiers

#### Scenario: Запись cache не удалась
- **WHEN** PostgreSQL вернул snapshot, но обновление Redis завершилось ошибкой
- **THEN** клиент получает успешный authoritative response
- **THEN** ошибка cache наблюдаема отдельно

### Requirement: Одновременные refresh одного аукциона объединяются
Auction SHALL объединять конкурентные L1 miss или stale refresh одного auction id, чтобы одна реплика выполняла не более одного downstream refresh для общего ожидания.

#### Scenario: Горячий ключ одновременно устарел
- **WHEN** несколько запросов одной реплики обнаруживают stale L1 для одного auction id
- **THEN** один запрос выполняет L2/fallback refresh
- **THEN** остальные получают тот же versioned result после завершения refresh

### Requirement: Price projector устойчив к повтору и перестановке
Auction-owned projector SHALL применять `PriceChanged` к Redis at-least-once через atomic compare-by-version и игнорировать повторные или более старые версии.

#### Scenario: События пришли не по порядку
- **WHEN** projector сначала применил version N, а затем получил version N-1
- **THEN** cache остаётся на version N

#### Scenario: Событие доставлено повторно
- **WHEN** тот же `PriceChanged` приходит повторно
- **THEN** cache state остаётся эквивалентным однократной доставке

### Requirement: Команды не зависят от публичного cache
Auction MUST загружать event stream или authoritative snapshot для `placeBid`, reschedule и lifecycle-команд и MUST NOT использовать L1/Redis current-price cache для проверки доменных инвариантов.

#### Scenario: Cache отстаёт от stream
- **WHEN** Redis показывает старую цену, а участник отправляет ставку
- **THEN** решение принимается по актуальному агрегату
- **THEN** mutation result отражает authoritative stream version, независимо от cache

### Requirement: Клиент может сохранять монотонное представление
Каждый current-price response и price update SHALL содержать stream version, чтобы BFF и Web могли не применять меньшую версию поверх уже увиденной большей.

#### Scenario: Query пришёл после более нового subscription update
- **WHEN** клиент уже применил version N, а refetch вернул version N-1
- **THEN** контракт позволяет клиенту проигнорировать устаревший snapshot
