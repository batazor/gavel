# 13. Workload and capacity model

Date: 2026-10-21

## Status

Accepted as the baseline capacity model. Числа пересматриваются по production telemetry и результатам нагрузочных тестов.

## Context

Gavel должен выдерживать:

- 10 миллионов новых лотов в сутки;
- 100 миллионов команд размещения ставки в сутки;
- design peak 2 000 ставок в секунду;
- десять клиентских наблюдений цены на одну ставку, то есть design peak 20 000 наблюдений в секунду;
- hot-lot skew: один лот может получить 1 миллион ставок, тогда как большинство не получает ни одной.

Фраза «пользователь проверяет цену десять раз» сама по себе не даёт RPS без количества пользователей. Для capacity model фиксируем точную интерпретацию: **10 price observations на одну команду ставки**. Price observation — клиентское получение актуальной цены через GraphQL query или subscription, а не обязательно отдельный запрос к Postgres. Доля business-rejected команд и технических retry пока неизвестна; `100M` — нагрузка на command interface, а объём сохранённых событий не может быть больше неё.

Среднее и peak нельзя смешивать:

| Поток | В сутки | Среднее | Design peak |
|---|---:|---:|---:|
| Новые лоты | 10 000 000 | `10M / 86400 = 116/s` | не задан |
| Команды ставки | 100 000 000 | `100M / 86400 = 1 157/s` | `2 000/s` |
| Наблюдения цены | 1 000 000 000 | `1B / 86400 = 11 574/s` | `20 000/s` |

Нагрузка не распределена равномерно по ключам. Партиционирование по `auction_id` масштабирует разные лоты, но не ускоряет один hot aggregate, которому всё равно нужен единый порядок ставок.

## Decision

### Одна системная модель, локальные решения

Этот ADR — единственный источник исходных объёмов и формул. Каждый проект получает производный capacity envelope ниже и ссылается на него из своего init ADR. Проект создаёт локальный ADR только при выборе собственной реализации горячего пути: например, Auction — сериализация и snapshots, BFF — fan-out/coalescing, Analytics — партиционирование ClickHouse.

Так числа не копируются и не расходятся между проектами, а локальные trade-offs остаются рядом с владельцем.

### Hot-lot envelope

Для hot lot задан объём `H = 1 000 000` ставок, но не задан интервал `T`. Нагрузка одного aggregate вычисляется как `H / T`:

| Миллион ставок за | Per-auction rate |
|---|---:|
| 24 часа | `11.6/s` |
| 1 час | `278/s` |
| 10 минут | `1 667/s` |
| 1 минуту | `16 667/s` |

До появления продуктового ограничения на `T` нагрузочные тесты обязаны проверять минимум два профиля: global peak `2 000 bid/s` по многим лотам и hot-key ramp до предела одного Auction stream. Найденный предел и требуемый hot-lot SLO фиксируются локальным ADR Auction; выдумывать его из суточного объёма нельзя.

### Capacity envelope по проектам

| Проект | Производная нагрузка | Что пока неизвестно |
|---|---|---|
| BFF / Cosmo + Realtime | `2 000 placeBid mutations/s`; до `20 000 price observations/s` через lightweight query или subscription. Subscription egress = `price events/s × subscribers(lot)`, поэтому fan-out может быть намного больше origin reads | full-screen query rate, concurrent subscriptions, watchers hot lot, query-plan cost |
| Authz/SpiceDB | До `4 000 permission checks/s` только для bid path, если edge pre-check и authoritative Auction check выполняются для каждой из `2 000` команд/с; public price/subscription checks не создают. Viewer capabilities используют BulkCheck | authenticated query RPS, cache hit ratio, relationship write rate, graph depth |
| Catalog | `116 ListLot/s` в среднем; 10M новых rows и `LotListed` в сутки | peak создания, search/list read ratio, media traffic |
| Auction | `2 000 PlaceBid evaluations/s` global peak; до `2 000 write transactions/s`, если все команды приняты; один упорядоченный stream на hot lot | rejection/retry rate, длительность hot-lot burst и требуемая per-key latency |
| Auction price cache | L1 сворачивает concurrent reads одного `auction_id` в один Redis refresh на 500 мс на реплику; Redis получает version-aware updates `PriceChanged` и miss fills, а не `20 000` обязательных reads/s | hot-key distribution, L1/L2 hit ratio, projector lag, Redis memory и eviction policy |
| auction-db | Если все команды приняты — до трёх private event rows (`MaxBidPlaced`, optional `PriceChanged`, optional `DeadlineExtended`) и до трёх public outbox rows (`BidRecorded` всегда + optional `PriceChanged`/`DeadlineExtended`) на ставку: worst case `600M appended rows/day`, `12 000 rows/s` при peak | acceptance rate, сериализованный размер event, snapshot interval, retention |
| Redpanda/outbox | До трёх публичных auction events на принятую ставку, то есть worst case `300M messages/day` и `6 000 messages/s` при peak, плюс lifecycle и catalog events | средний message size, replication, retention-bytes |
| Processes/Temporal | В среднем до `116 AuctionLifecycle starts/s`; до `2 000 DeadlineExtended signals/s`; settlement starts зависят от распределения дедлайнов | burst закрытий, доля отмен, число settlement rounds |
| Payments | `payment_attempts/s = closed_auctions/s × average_candidates_attempted`; registration traffic из исходных чисел не выводится | decline/recovery rate, registrations, provider latency/limits |
| Notify | `candidate_selected + payment_recovery + winner_confirmed`; технические retry увеличивают provider calls, но не logical notifications | channel mix, bounce rate, outbid notifications, provider limits |
| Analytics/ClickHouse | До `6 000 auction events/s` при worst-case peak плюс Catalog/Payments/Notifications; `BidRecorded` даёт полную историю, ingestion асинхронный | query concurrency, retention, serialized bytes |

### Lossless domain, coalescible UI

- Принятые ставки, приватные события Auction, outbox и публичные события в Redpanda не теряются и сохраняют порядок внутри `auction_id`.
- Realtime subgraph не обязан доставлять браузеру каждое промежуточное изменение цены hot lot. Последовательные `PRICE_CHANGED`/`DEADLINE_CHANGED` updates можно coalesce до последней версии для конкретного соединения.
- `closed`, `cancelled` и `paid` не coalesce и не отбрасываются.
- `afterVersion` указывает последнюю применённую версию. При reconnect Realtime subgraph может свернуть последовательность цен, но обязан сохранить terminal events и закончить replay актуальной ценой.
- Текущая цена читается из операционной проекции Auction; ClickHouse не входит в горячий путь.
- Root query `currentPrice` может обслуживаться version-aware L1/Redis cache согласно [Auction ADR-0002](../../contexts/auction/adr/0002-public-current-price-cache.md). Cache miss/error падает обратно на authoritative PostgreSQL-проекцию; `PlaceBid` cache не читает.

### Latency SLO

Под «проверить самую высокую цену» понимается чтение **текущей публичной цены**, а не чужого секретного максимума. Чтобы полный query plan не мешал этому SLO, Auction subgraph предоставляет отдельное root field `currentPrice(auctionId)`.

| Публичная операция | P99 SLO | Нагрузочный профиль |
|---|---:|---|
| Query `currentPrice(auctionId)` | `< 200 ms` | До `20 000 GraphQL reads/s`; subscription delivery latency измеряется отдельно |
| Query `lot(id)` для полного экрана | `< 500 ms` | Representative persisted-operation mix; целевой RPS ещё должен быть задан |
| Field `Auction.bidHistory(first, after)` | `< 500 ms` | Cursor pages from ClickHouse; deep pagination and hot lot with 1M rows |
| Mutation `placeBid(input, idempotencyKey)` | `< 500 ms` | Global peak `2 000 commands/s` по множеству лотов |

- End-to-end latency измеряется на Cloudflare edge от принятия запроса до полного HTTP-ответа и включает Cloudflare processing, Tunnel, Istio, Cosmo, внутренние вызовы и хранилища; last-mile сеть браузера исключается. Origin-only профиль на Istio используется отдельно для локализации bottleneck.
- SpiceDB checks входят в end-to-end latency соответствующей операции. Отдельно измеряются P50/P99 permission check, BulkCheck size, cache hit ratio и datastore revision lag; отдельный SLO принимается после benchmark, не вычитается произвольно из 500 мс.
- Threshold проверяется для завершённых ответов операции, а доля `5xx`, `429`, timeouts и rejected iterations контролируется отдельно. Нельзя выполнить latency SLO, отклоняя значимую часть нагрузки.
- Для hot lot целевой P99 ставки применяется только до подтверждённого benchmark-предела одного stream. Сам предел пока не задан и должен быть принят локальным ADR Auction.
- `PriceSnapshot.version` позволяет клиенту не заменить более новую цену старой; price read не использует Analytics и отдаётся с `Cache-Control: no-store`.

### Overload и проверка

- **Базовый генератор нагрузки — k6.** Сценарии будут храниться в `tests/load/` рядом с конфигурацией репозитория и запускаться одинаково локально и в CI/performance environment.
- Набор k6-сценариев разделяется по профилям: `global-bids` (много лотов, 2 000 mutations/s, P99 < 500 мс), `hot-lot` (ramp одного `auction_id`), `price-reads` (до 20 000 queries/s, P99 < 200 мс), `lot-screen` (P99 < 500 мс), `bid-history` (cursor pagination по лоту с 1M записей, P99 < 500 мс) и `subscription-fanout` (долгоживущие соединения, reconnect и медленные клиенты).
- Production-like k6 создаёт нагрузку через Cloudflare public hostname; origin-only профиль запускается отдельно и не считается доказательством end-to-end SLO. SpiceDB check latency/cache hit/datastore revision lag, broker lag, event-store throughput, Debezium connector/outbox lag, retained WAL, Temporal schedule-to-start и ClickHouse lag снимаются из telemetry системы, а не имитируются прямыми запросами к внутренним хранилищам.
- Если стандартного runner недостаточно для корректного GraphQL subscription/fan-out сценария, допускается k6 extension или отдельный специализированный driver; workload profile и критерии при этом остаются теми же.
- `Idempotency-Key` делает пользовательские повторы безопасными.
- До принятия команды BFF может применить backpressure/rate limit; принятую ставку нельзя молча потерять.
- Клиент получает явный `429` или `503` с retry semantics, а не ложный success.
- Нагрузочный тест отдельно измеряет global throughput, hot-key throughput, Cosmo/Realtime subscription fan-out, event-store/outbox growth, Debezium connector lag и retained WAL, broker lag, Temporal schedule-to-start latency и ClickHouse consumer lag.
- k6 thresholds фиксируют latency SLO выше; достижение одного целевого RPS без проверки ошибок и lag не считается успешным тестом.
- Error-rate SLO, full-screen RPS, concurrent users, payload bytes, retention и disaster-recovery объёмы пока не заданы; они не подменяются RPS и остаются открытыми входами capacity planning.

## Consequences

- Средний global RPS больше не используется как доказательство готовности к hot lot.
- Один hot aggregate остаётся последовательным bottleneck; увеличение числа Auction replicas само по себе его не лечит.
- Восстановление aggregate из миллиона событий на каждую команду неприемлемо: Auction должен выбрать и проверить локальную стратегию cache/snapshot без изменения event-sourced interface.
- GraphQL subscription защищает origin от polling, но переносит нагрузку в concurrent connections и fan-out; эти метрики становятся обязательными.
- Бюджеты Payments, Notifications и settlement нельзя получить из числа ставок без коэффициентов конверсии и отказов.
- Любое изменение исходных объёмов меняется здесь, после чего пересчитываются производные envelopes и нагрузочные профили.

## References

- [Auction init](../../contexts/auction/adr/0001-init.md)
- [BFF init](../../contexts/bff/adr/0001-init.md)
- [Federation composition](../../contexts/bff/cosmo/supergraph.yaml)
- [Processes init](../../contexts/processes/adr/0001-init.md)
- [Payments init](../../contexts/payments/adr/0001-init.md)
- [Analytics init](../../contexts/analytics/adr/0001-init.md)
- [ADR-0012 — Redpanda](0012-redpanda-as-message-broker.md)
- [ADR-0014 — Availability, replication and sharding](0014-availability-replication-and-sharding.md)
- [ADR-0016 — Cloudflare public edge](0016-cloudflare-as-public-edge.md)
