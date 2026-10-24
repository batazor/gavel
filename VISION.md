# Gavel — цели проекта

Учебно-исследовательский проект: аукцион, на котором обкатываем современный стек и практики.
Документ фиксирует, *что* хотим сделать. Детали каждого пункта — в ADR / спеках по мере принятия решений.

## Что строим

1. **Аукцион.** Английские восходящие торги с proxy bidding: лоты, ставки, завершение торгов, определение победителя ([Auction init](contexts/auction/adr/0001-init.md)).

## Как строим

2. **SDD через OpenSpec.** Сначала proposal и capability specs, затем design/tasks и код. OpenSpec changes живут в `openspec/changes/`, принятые спецификации — в `openspec/specs/`; они служат источником требований для тестов и доказательств. Репозиторий инициализирован с профилем `spec-driven`, артефакты пишутся на русском с сохранением структурных keywords OpenSpec.
3. **OpenTelemetry + Prometheus Operator + Grafana.** Traces и logs идут по OTLP через collector в Tempo/Loki; operational SLI и target health собирает Prometheus Operator, долговременные metrics уходят в Mimir. Alertmanager маршрутизирует page/warning/ticket, dashboards provisioned as code ([ADR-0017](docs/adr/0017-prometheus-operator-and-grafana.md)).
4. **Формальные доказательства в Lean.** Ключевые инварианты аукциона (например: победитель всегда платит не больше своей ставки; ставка ниже текущей не принимается; аукцион завершается ровно один раз) доказываются в Lean 4. Спека (п. 2) — источник утверждений для доказательств.
5. **DDD + микросервисы.** Один bounded context = один сервис + своя БД (ADR-0004). Агрегаты, доменные события, ubiquitous language. Домен не зависит от инфраструктуры.
6. **Rust** — бэкенд.
7. **Keploy** — попробовать для записи/воспроизведения интеграционных тестов. Эксперимент: проверить, заходит ли, и зафиксировать вывод.
8. **Authentication: OAuth/OIDC + JWT. Authorization: SpiceDB.** Внешний провайдер подтверждает `(iss, sub)`; mesh валидирует JWT. Resource permissions вычисляет Authz, роли из JWT не считаются источником прав ([Authz init](contexts/authz/adr/0001-init.md)).
9. **Astro** — фронт.
10. **Cloud native.** Контейнеры, Kubernetes, конфигурация через окружение, health/readiness, graceful shutdown, 12-factor.
11. **Layered traffic protection.** Cloudflare отсекает DDoS/bot/anonymous abuse на public edge; Cosmo ограничивает GraphQL operations; Istio применяет identity-aware rate limiting, mTLS, retries/timeouts и трассировку. Доменный код не содержит infrastructure counters ([ADR-0002](docs/adr/0002-rate-limiting-via-istio.md)).
12. **ADR** (Architecture Decision Records). Системные решения живут в `docs/adr/`, локальные — в `contexts/<project>/adr/` рядом с владельцем. Открытые вопросы ниже закрываются через ADR.
13. **Архитектура как код: LikeC4.** Модель в `docs/architecture/`, диаграммы генерируются, валидация в CI (ADR-0003).
14. **Event sourcing + transactional outbox через Debezium.** Auction — event sourced ([Auction init](contexts/auction/adr/0001-init.md)); Kafka Connect/Debezium читает только публичный outbox из PostgreSQL WAL и публикует его в брокер ([ADR-0005](docs/adr/0005-transactional-outbox-for-integration.md)).
15. **Realtime через GraphQL subscriptions.** Cosmo Router завершает публичный transport; отдельный Realtime subgraph восстанавливает поток с Auction version, затем переключается на Redpanda live events ([BFF init](contexts/bff/adr/0001-init.md)).
16. **Локальная разработка — kind.** Кластер с Istio, OTel, Postgres и брокером поднимается одним скриптом (ADR-0007).
17. **Деплой — Helm + ArgoCD.** GitOps с ветки `main`, руками в кластер не ходим (ADR-0007).
18. **CI экономный.** На PR — Buf format/lint/build/breaking, Federation composition, LikeC4, SpiceDB, Rust fmt/clippy и lint всех service-owned Helm charts. Тесты и Lean остаются тяжёлыми локальными шагами; matrix-сборка восьми images запускается вручную без push (ADR-0007).
19. **Feature flags — предусмотреть.** Точка расширения в коде закладывается сразу, провайдер выбираем позже.
20. **BFF — единственная публичная API-точка входа.** Self-hosted WunderGraph Cosmo Router исполняет federated GraphQL; owner subgraphs реализуют поля и команды, Realtime subgraph — replay/live semantics. Доменных правил в router нет ([BFF init](contexts/bff/adr/0001-init.md)).
21. **Postgres + ClickHouse.** Postgres для event store и операционных проекций, ClickHouse для аналитической истории, читает брокер напрямую ([Analytics init](contexts/analytics/adr/0001-init.md)).
22. **Temporal — слой процессов.** Факты идут в брокер, намерения во времени в Temporal: жизненный цикл аукциона, саги. Состояние торгов workflow не хранит ([Processes init](contexts/processes/adr/0001-init.md)).
23. **Redpanda как брокер, Kafka API.** Топики по контексту-владельцу, долгий retention для переигрывания проекций (ADR-0012).
24. **Политика окончания торгов задаётся продавцом.** Плановое время, окно бездействия, жёсткий потолок и их сочетание. Пересчёт в агрегате, Temporal только держит таймер. Доменное правило, описано в `contexts/auction/README.md`.
25. **Платежи — отдельный provider-neutral контекст.** Платёжный инструмент и разрешение на автоматическое списание готовятся до первой ставки. Stripe — первый сменный адаптер. После закрытия Temporal оркестрирует попытку, 2 часа recovery и переход к следующему кандидату с пересчитанной settlement amount; closing price не меняется ([Payments init](contexts/payments/adr/0001-init.md), [Processes init](contexts/processes/adr/0001-init.md)).
26. **Структура репозитория по DDD.** `contexts/<context>/` с README контекста и `use-cases/<case>/` (README + `flow.c4`); BFF и process worker там же, как часть нашей границы. ADR — только для архитектурных решений, доменные правила живут рядом с юзкейсами.
27. **Нагрузочный baseline, SLO и k6.** 10 млн новых лотов и 100 млн ставок в сутки; design peak 2 000 ставок и 20 000 наблюдений цены в секунду. Текущая публичная цена читается за P99 < 200 мс, полный экран и ставка — за P99 < 500 мс. k6 — базовый генератор для global, hot-lot, GraphQL query/mutation и subscription/fan-out профилей. Hot lot до 1 млн ставок тестируется отдельно, потому что global horizontal scaling не устраняет bottleneck одного aggregate ([ADR-0013](docs/adr/0013-workload-and-capacity-model.md)).
28. **Доступность 99,95%.** Критичные journeys имеют отдельный monthly SLO; точный годовой error budget — 4 ч 22 мин 48 с. Stateless-сервисы распределены по зонам, каждый production Postgres shard имеет leader и минимум две replicas с automatic failover. Актуальная цена и ставки идут в leader; stale-tolerant reads — в replicas с lag guard. Система shard-ready, physical shard count выбирается по benchmark ([ADR-0014](docs/adr/0014-availability-replication-and-sharding.md)).
29. **Полная публичная история ставок.** Каждая принятая ставка создаёт безопасный `BidRecorded` с alias внутри аукциона и результирующей публичной ценой. Максимум и `user_id` остаются в Auction; ClickHouse хранит долгую историю, Analytics subgraph отдаёт её cursor pages ([Analytics init](contexts/analytics/adr/0001-init.md)).
30. **Notify — отдельный provider-neutral контекст.** Processes решает, кого и когда уведомить; Notify владеет шаблоном, каналом, идемпотентностью, retry и историей доставки. Email provider — сменный адаптер ([Notify init](contexts/notify/adr/0001-init.md)).
31. **Fine-grained authorization через SpiceDB.** BFF делает UX pre-check, а сервис-владелец — authoritative check. Permissions не подменяют domain eligibility; ZedTokens защищают causal consistency после изменения relationships ([ADR-0015](docs/adr/0015-fine-grained-authorization-with-spicedb.md)).
32. **Cloudflare — обязательный public edge.** Web, `/graphql` и payment callbacks идут через Cloudflare и HA Cloudflare Tunnel к Istio; origin не имеет публичного входа. Edge отвечает за DNS/TLS, DDoS/WAF/bot controls и static asset cache, но не за authentication или authorization ([ADR-0016](docs/adr/0016-cloudflare-as-public-edge.md)).
32. **S3-compatible object storage для фотографий лотов.** Catalog владеет metadata и жизненным циклом изображений, Web загружает бинарные данные напрямую по presigned URL. Kind использует ephemeral in-process adapter, `s3s-fs` проверяет реальную S3 boundary в disposable integration tests, production provider сменный ([Catalog ADR-0002](contexts/catalog/adr/0002-s3-compatible-media-storage.md)).
33. **Storybook для Web UI.** Компоненты Astro разрабатываются и документируются изолированно в stories рядом с исходниками. Storybook покрывает визуальные состояния, accessibility и visual regression в CI; GraphQL-зависимости подменяются mocks. Это development tooling, поэтому в runtime C4-модель не входит и не заменяет integration/E2E-тесты.
34. **Version-aware кэш публичной текущей цены.** Root query `currentPrice` использует L1 in-memory cache с freshness window 500 мс, Redis L2 и fallback на authoritative PostgreSQL-проекцию. В кэш никогда не попадают секретные максимумы, а `PlaceBid` его не читает; обновления применяются только по возрастающей Auction stream version ([Auction ADR-0002](contexts/auction/adr/0002-public-current-price-cache.md)).
35. **Workflow observability.** Для Temporal/Processes измеряем availability, worker→Temporal long-poll latency, schedule-to-start, pollers/slots, unexpected terminal failures и просроченное закрытие аукциона. Нормальные durable timers не считаются зависанием; customer-impact pages имеют runbook ([Processes ADR-0002](contexts/processes/adr/0002-workflow-observability.md)).
36. **Внутренний RPC — gRPC + Buf.** Синхронные межконтекстные command/query interfaces принадлежат владельцам и версионируются как отдельные Buf modules; браузер остаётся на GraphQL, факты — в Redpanda. Git — source of truth, BSR возможен позже как registry RPC sources/SDK и не является registry runtime events ([ADR-0018](docs/adr/0018-internal-rpc-with-grpc-and-buf.md)).

Список юзкейсов и их статусы: `contexts/README.md`.

## Кандидаты на «что-то ещё» (не решено)

- **Формат и реестр схем событий.** JSON Schema или Protobuf и политика Redpanda Schema Registry для runtime event contracts. Это отдельное решение от принятого Buf/возможного BSR для RPC sources.
- **Идемпотентность ставок.** Idempotency key на POST. Конкуренция решена версией потока событий ([Auction init](contexts/auction/adr/0001-init.md)).
- **Property-based тесты (proptest)** как мост между Rust-кодом и Lean-доказательствами: те же инварианты проверяются на случайных данных.
- **PostHog.** Продуктовая аналитика в перспективе, не сейчас.
- **Провайдер feature flags.** OpenFeature как абстракция, провайдер позже.

## Вне скоупа

- Выплаты продавцам, возвраты, эскроу. После базовой оплаты, отдельные ADR и provider-neutral контракты.

## Открытые вопросы

- Как связать Lean и Rust: ручное соответствие по спеке, или генерация/извлечение кода?
