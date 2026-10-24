# Catalog

Описание лотов: название, фото, категория, стартовая цена, политика окончания. Read-heavy, торговой логики нет.

## ADR

- [0001 — Инициализация проекта](adr/0001-init.md)
- [0002 — S3-compatible storage для медиа](adr/0002-s3-compatible-media-storage.md)

## Interface

- [Federation subgraph SDL](graphql/schema.graphql) — владеет `Lot`, catalog queries и mutations; публикуется только через Cosmo Router.
- [gRPC contract](proto/gavel/catalog/v1/catalog_service.proto) — внутренняя идемпотентная команда снятия лота от Processes; правила — в [init ADR](adr/0001-init.md).
- GraphQL `listLot` выводит стабильный `lot_id` из verified principal и idempotency key; server timestamp не меняет canonical intent точного повтора.

## Язык
| Термин | Значение |
|---|---|
| Лот | Предмет торгов с описанием. Принадлежит продавцу |
| Изображение лота | Управляемый Catalog медиаобъект; бинарные данные лежат в S3-compatible storage |
| Выставить | Создать лот и запустить торги |
| Снять | Убрать лот с торгов. Возможно только без ставок |

## Агрегат `Lot`
Обычное состояние в Postgres, без event sourcing. Outbox — рядом с таблицей лота, в той же транзакции.

**Команды:** `RequestLotImageUpload`, `CompleteLotImageUpload`, `ListLot`, `UpdateLot` (до первой ставки), `WithdrawLot`.
**События:** `LotListed(lot_id, seller, start_price, step, closing_policy)`, `LotUpdated`, `LotWithdrawn`.

## Инварианты
- Лот содержит от 1 до 20 изображений в заданном продавцом порядке.
- К лоту можно прикрепить только `READY`-изображение этого продавца; после прикрепления повторное использование запрещено.
- Описание замораживается после первой ставки (узнаём по `PriceChanged` из брокера или через сагу).
- Снять можно только лот без ставок: это проверяет Auction, Catalog доверяет саге.

## Интеграции
- `LotListed` → брокер → Auction открывает торги.
- Снятие — сага `WithdrawLot` в Temporal: сначала `CancelAuction` в Auction, потом `WithdrawLot` здесь.
- Метаданные и связи: `catalog-db`; бинарные объекты: S3-compatible storage через сменный adapter.
- Web загружает бинарные данные напрямую по presigned `PUT`; Router и catalog-service не проксируют тело файла.

## Юзкейсы
- [list-lot](use-cases/list-lot/README.md)
- [upload-lot-image](use-cases/upload-lot-image/README.md)
- [withdraw-lot](use-cases/withdraw-lot/README.md)

## Operations

- [Tokio runtime](src/main.rs) поднимает HTTP `/graphql`, `/healthz`, `/readyz`, `/metrics` и внутренний gRPC `WithdrawLot`; readiness включается только после подключения PostgreSQL, Authz и S3-compatible storage.
- Runtime супервизирует lease-based media cleanup, Authz-grant reconciliation и обновление backlog gauges; аварийное завершение worker делает pod unready и завершает процесс.
- `make database-check`, `make storage-check` и `make integration-check` запускают pinned disposable PostgreSQL/s3s-fs и реальный boundary flow.
- [Docker image, Helm chart, Prometheus rules и Grafana dashboard](ops/Helm/README.md)

Метрики и structured logs используют только bounded `operation/outcome`. Principal/lot/image ids, object keys, presigned URLs, checksums, HMAC material, causal tokens и credentials редактируются и запрещены в labels.
