# Auction

Ядро системы. Торги по лоту: ставки, лидер, цена, дедлайн, закрытие, победитель. Event sourced, инварианты доказываются в Lean.

## ADR

- [0001 — Инициализация проекта](adr/0001-init.md)
- [0002 — Version-aware cache публичной текущей цены](adr/0002-public-current-price-cache.md)

## Interface

- [Federation subgraph SDL](graphql/schema.graphql) — executable runtime владеет `Auction`, authoritative current price и bid mutations, а extension `Lot.auction` разрешает через owner-backed проекцию по `lot_id`; публикуется только через Cosmo Router.
- [gRPC contract](proto/gavel/auction/v1/auction_service.proto) — внутренние команды Processes: close, cancel, forfeiture и confirm winner; правила — в [init ADR](adr/0001-init.md).
- [Public event contract](proto/gavel/auction/v1/auction_events.proto) — versioned безопасные события для Redpanda; секретные максимумы, `user_id` и внутренние storage keys в контракт не входят.
- [Restricted process-event contract](proto/gavel/auction/v1/auction_process_events.proto) — отдельный versioned поток только для trusted Processes workload; содержит текущего settlement candidate и amount, но никогда не публикуется realtime/analytics consumers.

## Язык

Канонические определения также собраны в [глоссарии контекста](GLOSSARY.md).

| Термин | Значение |
|---|---|
| Аукцион | Торги по одному лоту. Английский, восходящий, с автоставками ([init](adr/0001-init.md)) |
| Максимум | Секретная верхняя граница участника. Наружу не выходит |
| Псевдоним участника | Стабильный только внутри одного аукциона публичный alias; не раскрывает `user_id` и не связывает человека между лотами |
| Лидер | Участник с наибольшим максимумом; при равенстве — кто раньше |
| Текущая цена | `min(max_лидера, max_второго + шаг)`; без второго — стартовая |
| Шаг | Минимальное приращение цены |
| Дедлайн `ends_at` | Момент, после которого аукцион можно закрыть. Вычисляется политикой окончания |
| Settlement candidate | Текущий лидер после закрытия, которому предложена оплата; ещё не подтверждённый победитель |
| Settlement amount | Сумма предложения текущему кандидату, рассчитанная pricing policy без выбывших участников |
| Победитель | Settlement candidate с успешно подтверждённой оплатой |

## Агрегат `Auction`

Состояние — свёртка событий потока `auction-<id>`. Версия потока = optimistic lock ([init](adr/0001-init.md)).

**Команды:** `OpenAuction(lot_id, start_price, step, closing_policy)`, `PlaceBid(user, max)`, `RescheduleAuction(seller, scheduled_end)`, `CloseAuction(now)`, `CancelAuction`, `ForfeitSettlementCandidate(user, round)`, `ConfirmWinner(user, payment_id)`.

**События (приватные, в журнале):** `AuctionOpened`, `MaxBidPlaced(user, max)`, `DeadlineExtended(ends_at)`, `AuctionRescheduled(ends_at)`, `AuctionClosed`, `AuctionCancelled`, `SettlementCandidateSelected(user, price, round)`, `SettlementCandidateForfeited`, `WinnerConfirmed(user, payment_id)`, `AuctionUnsold`.

**Публичная проекция (в брокер и ClickHouse):** для каждой принятой команды ставки `BidRecorded(bid_id, auction_id, bidder_alias, resulting_price, occurred_at, version)`. `resulting_price` — публичная цена после ставки, не секретный максимум. Если цена изменилась, дополнительно публикуется `PriceChanged(price, ends_at, version)` для realtime. Идентификаторы пользователя и лидера наружу не выходят.

## Политики (value objects внутри агрегата)

### Pricing policy
Чистая функция `(максимумы, шаг) → (лидер, цена)`. Пересчёт при каждой принятой ставке, синхронно, без планировщика. Это политика агрегата, не доменный сервис: работает только над состоянием одного аукциона.

### Closing policy
Задаёт продавец при выставлении лота:

```
ClosingPolicy {
  scheduled_end: Option<Instant>,   // плановое окончание
  inactivity:    Option<Duration>,  // окно бездействия после последней ставки
  hard_deadline: Option<Instant>,   // потолок, дальше не продлевается
}
```

Хотя бы одно из `scheduled_end`, `inactivity` обязательно. Если задано только `inactivity`, окно отсчитывается от открытия, пока ставок нет.

```
ends_at = min(hard_deadline, max(scheduled_end, last_bid_at + inactivity))
```

Отсутствующие поля не участвуют. Anti-sniping — это `scheduled_end` + короткое `inactivity`. При каждой принятой ставке дедлайн пересчитывается; если сдвинулся — `DeadlineExtended`. Тип политики можно менять только до первой ставки, `scheduled_end` — в любой момент, пока торги открыты.

**Время — вход агрегата.** `CloseAuction(now)` приносит время с собой, агрегат часов не читает. Таймер держит Temporal (`processes/`), решения о продлении принимает агрегат.

## Инварианты (для Lean)

1. Текущая цена торгов не убывает, пока аукцион открыт. После закрытия она фиксируется как closing price.
2. Лидер держит наибольший максимум.
3. Цена ≤ второй максимум + шаг.
4. Победитель платит не больше своего максимума.
5. Аукцион закрывается ровно один раз и только при `now ≥ ends_at`.
6. `ends_at` никогда не превышает `hard_deadline`.
7. После закрытия новые ставки и перенос дедлайна запрещены; settlement-состояние меняется только командами `ForfeitSettlementCandidate` и `ConfirmWinner`.
8. Результат proxy-механики совпадает с честным последовательным перебиванием.
9. Settlement candidate имел готовый способ оплаты в момент принятия своей ставки.
10. Выбывший кандидат больше не выбирается; settlement amount следующего пересчитывается без его максимума и не меняет closing price.
11. Победитель подтверждается только после `PaymentSucceeded`; на аукционе не более одного победителя.

«Дедлайн не убывает» инвариантом **не** является: продавец вправе сократить.

## Интеграции

- Вход: команды от BFF (`PlaceBid`, `RescheduleAuction`) и от process worker (`CloseAuction`, `CancelAuction`, `ForfeitSettlementCandidate`, `ConfirmWinner`). Открытие — по `LotListed` из Catalog; готовность участника — по `PaymentInstrumentReady`/`PaymentInstrumentRevoked` из Payments.
- Выход: public outbox → `gavel.auction.public.v1` для Realtime/ClickHouse; отдельный restricted process outbox → ACL-protected `gavel.auction.processes.v1` только для Processes. Обе записи атомарны с private stream, но контракты и consumer ACL не смешиваются; canonical metadata и partition key описаны в корневом `contracts/event-topics.json`.
- Хранилище: `auction-db` (Postgres): authoritative журнал, outbox и проекции. Redis хранит только восстанавливаемый L2-cache публичной текущей цены; L1 живёт в памяти реплики.
- `PlaceBid` и другие команды никогда не читают price cache. Root query `currentPrice` использует version-aware L1 → Redis → PostgreSQL fallback.

## Юзкейсы

- [place-bid](use-cases/place-bid/README.md) — ставка
- [get-current-price](use-cases/get-current-price/README.md) — публичная цена через L1/L2-cache
- [proxy-bid](use-cases/proxy-bid/README.md) — автоставка
- [close-by-deadline](use-cases/close-by-deadline/README.md) — закрытие по дедлайну
- [reschedule](use-cases/reschedule/README.md) — продавец меняет время окончания

Открытие торгов описано в [catalog/use-cases/list-lot](../catalog/use-cases/list-lot/README.md), оно начинается там.

## Operations

- [Чистый event-sourced доменный слой](src/domain/) — агрегат, команды, приватные события, безопасные публичные события и value objects без инфраструктурных зависимостей
- [Application layer](src/application/) — единый command executor с idempotency fingerprint/result, bounded optimistic retry, Authz/payment checks и post-commit reconciliation
- [Adapters](src/adapters/) — GraphQL/gRPC/event consumers, PostgreSQL unit of work и projections, Redis L2 и Authz gRPC client
- GraphQL adapter выводит `bid_id` детерминированно из verified principal, `auction_id` и idempotency key. Серверное время не входит в canonical fingerprint, поэтому точный retry возвращает сохранённый результат и не создаёт вторую ставку.
- [Runtime](src/bootstrap.rs) — HTTP `/healthz`, `/readyz`, `/metrics`, `/graphql`, отдельный internal gRPC listener, supervised workers и graceful shutdown
- Миграции запускаются отдельной командой `gavel-auction migrate`; runtime не применяет schema changes при старте. Секреты передаются через external secret references Helm chart.
- `make -C contexts/auction check`, `database-integration`, `cache-integration`, `integration-check` и `observability-check` — локальные quality gates
- [Docker image, Helm chart, Prometheus rules и Grafana dashboard](ops/Helm/README.md)
