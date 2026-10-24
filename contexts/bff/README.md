# BFF

Единственная публичная API-точка Gavel — self-hosted **WunderGraph Cosmo Router**. Он исполняет federated GraphQL, а не содержит доменные правила. Каждый bounded context владеет своим subgraph interface; BFF скрывает topology и предоставляет Web один graph.

## ADR

- [0001 — Инициализация BFF на WunderGraph Cosmo Router](adr/0001-init.md)

## Исследования

- [Apollo Router как BFF для Gavel](research/apollo-router-evaluation.md)
- [Self-hosted alternatives и CNCF gateways](research/self-hosted-router-alternatives.md)

## Публичный interface

- Endpoint MVP: `POST /graphql`; Web обновляет `LotWorkspace` version-aware polling-ом. Subscription transport и Realtime subgraph сохранены как следующий этап.
- MVP GraphQL queries компонуют owner-backed данные Catalog и Auction; Payments подключён как service-owned subgraph для персональных payment operations.
- GraphQL mutations вызывают owner subgraph; идемпотентность и authoritative result обеспечивает владелец команды.
- Realtime subscription contract сохранён как deferred fixture и не входит в MVP graph/readiness.
- Public reads доступны без JWT; protected operations и персональные поля требуют verified principal.
- Provider callbacks не входят в graph: Cloudflare/Istio направляют отдельный path в Payments.
- Cosmo Router обращается к owner subgraphs только по GraphQL/HTTP. Внутренние [gRPC endpoints](../../docs/adr/0018-internal-rpc-with-grpc-and-buf.md) предназначены для workload-to-workload команд, не маршрутизируются Router и не публикуются браузеру.

Схема compose-ится из SDL, которыми владеют проекты. Скомпонованный execution config генерируется и проверяется в delivery pipeline; его не редактируют вручную.

## Modules и interfaces

- **Cosmo Router** — validation, persisted operations, query planning, composition, traffic controls и OTLP telemetry.
- **Context subgraphs** — единственные владельцы resolvers, доменных команд и resource authorization.
- **Realtime subgraph** — последующий узкий adapter; его schema не включена в MVP composition.
- **Cloudflare + Tunnel** — обязательный public edge, DDoS/WAF/bot controls и origin protection; `/graphql` не кэшируется.
- **Istio ingress** — JWT verification, identity-aware rate limiting и маршрут `/graphql`; он не выполняет Federation composition.

## Юзкейсы

- [live-updates](use-cases/live-updates/README.md) — GraphQL subscription, replay после reconnect и hot-lot semantics

Authorization schema и ownership отношений описаны в [Authz](../authz/README.md).

## Operations

- [Pinned Cosmo Router image, Helm chart, Prometheus rules и Grafana dashboard](ops/Helm/README.md)

## Web operations

Проверяемые MVP operations хранятся в [`operations/`](operations/). `LotWorkspace` компонует карточку Catalog и публичное состояние Auction, а `PlaceBid` передаёт идемпотентную команду authoritative Auction subgraph. Web знает только этот interface и не обращается к subgraphs напрямую. Будущий `AuctionUpdates(afterVersion)` сохранён в [`fixtures/deferred/`](fixtures/deferred/), но не валидируется как доступный MVP operation.

```bash
npm run graphql:operations:check
```

Команда сначала собирает Federation graph, затем валидирует все operation documents против публичной схемы из execution config. Для локального подключения Web задайте `VITE_GRAPHQL_URL`; без него UI явно показывает `Demo data`, сохраняя возможность разрабатывать интерфейс без запущенного Router.

MVP использует polling по Auction stream version. GraphQL subscriptions и Analytics history будут возвращены отдельным change после готовности replay/live seam.
