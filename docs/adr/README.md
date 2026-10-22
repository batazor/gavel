# Системные ADR

Здесь живут только решения, которые затрагивают несколько проектов, общую архитектуру, delivery или весь репозиторий. Решения одного проекта находятся в `contexts/<project>/adr/`; исходное решение проекта — в `0001-init.md`.

| ADR | Почему системный |
|---|---|
| [0001 — Record architecture decisions](0001-record-architecture-decisions.md) | Задаёт правила ADR для всего репозитория |
| [0002 — Rate limiting via Istio](0002-rate-limiting-via-istio.md) | Mesh-политика применяется ко всем сервисам |
| [0003 — Architecture as code with LikeC4](0003-architecture-as-code-likec4.md) | Описывает общую модель и CI |
| [0004 — Service per bounded context](0004-service-per-bounded-context.md) | Задаёт границы и владение данными между проектами |
| [0005 — Transactional outbox via Debezium](0005-transactional-outbox-for-integration.md) | Задаёт межпроектную доставку событий через Kafka Connect |
| [0007 — Delivery](0007-delivery-kind-helm-argocd-lean-ci.md) | Определяет общее dev-, CI- и deployment-окружение |
| [0012 — Redpanda](0012-redpanda-as-message-broker.md) | Общий транспорт событий для нескольких проектов |
| [0013 — Workload and capacity model](0013-workload-and-capacity-model.md) | Общие объёмы и производные capacity envelopes всех проектов |
| [0014 — Availability, replication and sharding](0014-availability-replication-and-sharding.md) | Общий availability SLO и production HA topology всех stateful проектов |
| [0015 — Fine-grained authorization with SpiceDB](0015-fine-grained-authorization-with-spicedb.md) | Общая модель permissions и enforcement для всех проектов |
| [0016 — Cloudflare as the mandatory public edge](0016-cloudflare-as-public-edge.md) | Общий внешний ingress, DDoS/WAF, origin protection и subscription transport |
| [0017 — Prometheus Operator and Grafana](0017-prometheus-operator-and-grafana.md) | Общие target discovery, rule evaluation, alert routing и dashboard provisioning; конкретные SLI остаются у проектов |
| [0018 — Internal RPC with gRPC and Buf](0018-internal-rpc-with-grpc-and-buf.md) | Единые transport boundaries, ownership, compatibility и reliability rules для внутренних синхронных вызовов |

Если решение можно изменить вместе с одним проектом, не меняя интерфейсы остальных, оно должно находиться у этого проекта, а не здесь.
