# BFF

Единственная публичная API-точка Gavel — self-hosted **WunderGraph Cosmo Router**. Он исполняет federated GraphQL, а не содержит доменные правила. Каждый bounded context владеет своим subgraph interface; BFF скрывает topology и предоставляет Web один graph.

## ADR

- [0001 — Инициализация BFF на WunderGraph Cosmo Router](adr/0001-init.md)

## Исследования

- [Apollo Router как BFF для Gavel](research/apollo-router-evaluation.md)
- [Self-hosted alternatives и CNCF gateways](research/self-hosted-router-alternatives.md)

## Публичный interface

- Endpoint: `POST /graphql`; subscription transport также завершается на Cosmo Router.
- GraphQL queries компонуют данные Catalog, Auction, Analytics и Payments.
- GraphQL mutations вызывают owner subgraph; идемпотентность и authoritative result обеспечивает владелец команды.
- GraphQL subscriptions скрывают replay и live delivery за Realtime subgraph.
- Public reads доступны без JWT; protected operations и персональные поля требуют verified principal.
- Provider callbacks не входят в graph: Cloudflare/Istio направляют отдельный path в Payments.
- Cosmo Router обращается к owner subgraphs только по GraphQL/HTTP. Внутренние [gRPC endpoints](../../docs/adr/0018-internal-rpc-with-grpc-and-buf.md) предназначены для workload-to-workload команд, не маршрутизируются Router и не публикуются браузеру.

Схема compose-ится из SDL, которыми владеют проекты. Скомпонованный execution config генерируется и проверяется в delivery pipeline; его не редактируют вручную.

## Modules и interfaces

- **Cosmo Router** — validation, persisted operations, query planning, composition, traffic controls и OTLP telemetry.
- **Context subgraphs** — единственные владельцы resolvers, доменных команд и resource authorization.
- **Realtime subgraph** — узкий adapter: replay от `afterVersion`, переход на Redpanda live stream, coalescing price updates и гарантированная передача terminal events.
- **Cloudflare + Tunnel** — обязательный public edge, DDoS/WAF/bot controls и origin protection; `/graphql` не кэшируется.
- **Istio ingress** — JWT verification, identity-aware rate limiting и маршрут `/graphql`; он не выполняет Federation composition.

## Юзкейсы

- [live-updates](use-cases/live-updates/README.md) — GraphQL subscription, replay после reconnect и hot-lot semantics

Authorization schema и ownership отношений описаны в [Authz](../authz/README.md).

## Operations

- [Hello-world build scaffold](src/main.rs)
- [Docker image, Helm chart, Prometheus rules и Grafana dashboard](ops/Helm/README.md)
