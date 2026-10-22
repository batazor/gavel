# Analytics

Аналитическая история торгов в ClickHouse. Только публичные неизменяемые события, eventually consistent, не на горячем пути.

## ADR

- [0001 — Инициализация проекта](adr/0001-init.md)

## Interface

- [Federation subgraph SDL](graphql/schema.graphql) — read-only cursor history и price chart; current price здесь не определяется.

## Источник данных
Debezium PostgreSQL connectors доставляют публичные outbox-события владельцев в Redpanda. ClickHouse Kafka engine читает топики `auction.events`, `catalog.events`, `payments.events` в MaterializedView. Отдельного сервиса-consumer и Kafka Connect sink нет. Приватные события (`MaxBidPlaced`) сюда не попадают по построению.

## Потребители
- Grafana, ClickHouse datasource: дашборды торгов.
- Cosmo Router через Analytics subgraph: график цены и курсорная полная история ставок конкретного аукциона.

## Юзкейсы
- [price-history](use-cases/price-history/README.md)
- [bid-history](use-cases/bid-history/README.md)

## Operations

- [Hello-world build scaffold](src/main.rs)
- [Docker image, Helm chart, Prometheus rules и Grafana dashboard](ops/Helm/README.md)
