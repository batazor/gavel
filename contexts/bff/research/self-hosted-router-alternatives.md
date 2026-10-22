# Self-hosted alternatives to Apollo Router

Date: 2026-10-22

## Вопрос

Есть ли у Gavel полностью self-hosted альтернатива Apollo GraphOS Router, желательно из CNCF, которая заменит собственный BFF: даст GraphQL Federation/query composition, интеграцию с существующими REST/OpenAPI сервисами, subscriptions и расширяемую авторизацию через SpiceDB?

## Краткий вывод

Готового **CNCF-hosted GraphQL Federation router** уровня Apollo Router сейчас нет. Envoy/Envoy Gateway, kgateway и Higress решают L4/L7 routing, ingress, TLS, retries, rate limiting и external authorization, но не строят GraphQL query plan по supergraph и не собирают один GraphQL response из нескольких subgraphs. Их можно поставить **перед** специализированным GraphQL router, но нельзя считать заменой BFF composition layer.

Реальный self-hosted shortlist для Gavel:

1. **WunderGraph Cosmo Router** — основной кандидат для spike. Apache-2.0, compiled Go data plane, Federation и subscriptions; Cosmo Streams напрямую работает с Kafka/NATS/Redis, дедуплицирует одинаковые subscriptions и лучше всего совпадает с Redpanda/hot-lot архитектурой Gavel.
2. **The Guild Hive Gateway** — второй кандидат. MIT, автономный runtime, Federation, все основные subscription transports, удобная интеграция OpenAPI через GraphQL Mesh и внешний policy service через `@policy`/plugins.
3. **Grafbase Gateway** — технически подходил, но исключён: после приобретения The Guild официальный репозиторий переведён в maintenance mode и объявлен к архивированию к концу мая 2026.

Ни один из кандидатов не даёт документированных Gavel semantics `Last-Event-ID`, replay по версии Auction stream и business-aware coalescing terminal/price events. Эта часть остаётся в Auction/Realtime boundary и должна быть доказана отдельным spike независимо от выбранного router.

## Что означает «CNCF-альтернатива»

Нужно различать три разных статуса:

- **CNCF-hosted project** — проект принят CNCF и имеет официальный maturity level: Sandbox, Incubating или Graduated.
- **CNCF member** — организация, вступившая в CNCF; это ничего не говорит о governance или зрелости конкретного продукта.
- **CNCF Landscape entry** — запись в отраслевом каталоге. Само присутствие в Landscape не делает продукт CNCF-hosted project.

Официальный каталог CNCF явно разделяет hosted projects по maturity level и отдельно ссылается на Landscape как на более широкий обзор продуктов: [CNCF Projects](https://www.cncf.io/projects/).

### CNCF gateway projects

| Проект | Фактический CNCF-статус | Что умеет | GraphQL Federation/query composition | Роль в Gavel |
|---|---|---|---|---|
| Envoy Proxy | CNCF Graduated с 2018 года | Высокопроизводительный edge/service proxy | Нет supergraph planner/composer | Data plane либо внешний ingress |
| Envoy Gateway | Часть Envoy ecosystem/project; отдельного maturity entry в каталоге CNCF нет | Kubernetes Gateway API, HTTP/gRPC routing, retries, limits, JWT/ext-auth и extension points | Нет нативного Federation query planning | Предпочтительный внешний ingress перед GraphQL router, если нужен единый north-south слой |
| kgateway | CNCF Sandbox с 4 марта 2025 | Envoy-based Kubernetes API gateway, Gateway API, traffic/security policies | Нет документированной Federation composition | Возможная замена Envoy Gateway как ingress, но не BFF |
| Higress | CNCF Sandbox с 15 марта 2026 | Envoy/Istio-based API/AI gateway, Wasm plugins | Нет Federation composition; `de-graphql` только отображает REST URI в запрос к одному GraphQL upstream | Для Gavel не даёт преимущества перед Envoy Gateway |

Источники:

- [Envoy в CNCF: Graduated](https://www.cncf.io/projects/envoy/)
- [Envoy Gateway overview и feature surface](https://gateway.envoyproxy.io/docs/)
- [Envoy Gateway как часть Envoy project](https://www.cncf.io/blog/2022/05/16/introducing-envoy-gateway/)
- [kgateway в CNCF: Sandbox](https://www.cncf.io/projects/kgateway/)
- [kgateway documentation](https://kgateway.dev/docs/)
- [Higress в CNCF: Sandbox](https://www.cncf.io/projects/higress/)
- [Higress overview](https://higress.cn/en/docs/latest/overview/what-is-higress/)
- [Higress `de-graphql`](https://higress.cn/docs/latest/plugins/transformation/de-graphql/)

### Почему обычный API gateway не заменяет Federation router

Gateway API routes выбирают backend по listener, hostname, path, headers и protocol. GraphQL Federation router должен дополнительно:

1. разобрать и валидировать GraphQL operation;
2. сопоставить поля operation с владельцами в supergraph;
3. построить зависимый query plan с entity representations;
4. параллельно/последовательно вызвать несколько subgraphs;
5. объединить частичные данные и GraphQL errors в один ответ;
6. повторять entity resolution для каждого subscription event.

Envoy-класс отлично решает внешний transport/security layer, но перечисленной семантики в его документированном feature surface нет. Попытка реализовать её через Wasm/ext-proc фактически означает написать собственный Federation router внутри proxy.

## Матрица настоящих self-hosted GraphQL routers

| Критерий | Cosmo Router | Hive Gateway | Grafbase Gateway |
|---|---|---|---|
| CNCF status | Не CNCF project | Не CNCF project | Не CNCF project |
| Runtime | Go | JavaScript/TypeScript runtime; binary/Docker/npm distribution | Rust |
| Лицензия | Apache-2.0 | MIT | MPL-2.0 |
| Federation | v1/v2, собственный query planner | v1/v2, local или registry supergraph | Federation v2 |
| SaaS/control plane обязателен | Нет: весь stack self-hostable; router может читать static execution config | Нет: local supergraph; Hive Console также self-hostable | Нет: air-gapped local schema был доступен |
| Subscriptions к client | WebSocket, SSE, multipart HTTP | WebSocket, SSE, multipart HTTP | WebSocket, SSE, multipart HTTP |
| Интеграция event broker | Cosmo Streams: Kafka, NATS, Redis; subscription deduplication | Через subscription subgraph/HTTP callback или custom plugin | Через subgraph/extension |
| REST/OpenAPI как upstream | Нет прямого OpenAPI-to-subgraph import; нужны adapter/subgraph или custom module | Да: Mesh Compose превращает OpenAPI/REST sources в Federation-compatible schema | Да: declarative REST resolver extension |
| REST/OpenAPI наружу | Cosmo ConnectRPC может публиковать trusted GraphQL operations как HTTP/JSON и OpenAPI | SOFA plugin может публиковать GraphQL operations как REST/Swagger | Не ключевая возможность для решения Gavel |
| JWT/basic field auth | Built-in JWKS, `@authenticated`, `@requiresScopes` | JWT/JWKS, `@authenticated`, `@requiresScopes`, `@policy` | JWT/auth extensions, authorization directives |
| Custom SpiceDB pre-check | Custom Go module/build | Async policy fetch или JS/TS lifecycle plugin | Rust/Wasm authorization extension с разрешённым network access |
| Полностью пригоден на 2026-10-22 | Да, требует spike | Да, требует spike | **Нет: maintenance/EOL** |

## 1. WunderGraph Cosmo Router

Cosmo — полностью open-source Federation platform и router под Apache-2.0. Весь control plane можно развернуть on-premises, но для Gavel не обязательно сразу поднимать полный Studio stack: router поддерживает static execution config, созданный локальной composition. Это позволяет держать request data plane и schema delivery без внешнего SaaS.

Источники:

- [Cosmo repository и self-hosted overview](https://github.com/wundergraph/cosmo)
- [Apache-2.0 license](https://github.com/wundergraph/cosmo/blob/main/LICENSE)
- [Cosmo self-hosting](https://cosmo-docs.wundergraph.com/self-hosted)
- [Federation compatibility matrix](https://cosmo-docs.wundergraph.com/federation/federation-compatibility-matrix)

### Realtime fit

Обычные subscriptions поддерживают WebSocket, SSE и multipart HTTP как между client/router, так и между router/subgraph. При обновлении runtime config активные subscriptions завершаются, поэтому клиент обязан reconnect/resubscribe.

Cosmo Streams интереснее для Gavel: Router сам подключается к Kafka/NATS/Redis, дедуплицирует одинаковые topic subscriptions, фильтрует события и после trigger может дозапросить поля сущности из обычных subgraphs. Документация заявляет десятки тысяч соединений/событий, но это vendor claim, а не доказательство наших SLO; профиль с одним hot lot всё равно проверяется k6.

Источники:

- [Cosmo subscriptions и transports](https://cosmo-docs.wundergraph.com/router/subscriptions)
- [Cosmo Streams: Kafka/NATS/Redis и deduplication](https://cosmo-docs.wundergraph.com/router/cosmo-streams)

Ограничения для Gavel:

- В Cosmo Streams не документирован cursor replay/`Last-Event-ID`; после reconnect нельзя считать delivery восстановленным без отдельного доказательства.
- Topic deduplication не равно business coalescing: правило «промежуточную цену можно схлопнуть, terminal event терять нельзя» остаётся нашей логикой.
- Документация Cosmo Streams прямо указывает, что router сам не проверяет, имеет ли конкретный client право подписываться на topic. Для `auction:{id}` нужен custom subscription hook/Go module и authoritative checks в Auction; одной декларативной фильтрации недостаточно.
- Интеграция Redpanda вероятна через Kafka protocol, но должна быть проверена отдельным compatibility test, а не принята из названия Kafka integration.

### REST и SpiceDB

Cosmo не даёт аналога GraphQL Mesh/Apollo Connectors для автоматического импорта существующего OpenAPI как subgraph. `Cosmo ConnectRPC` решает обратную задачу: публикует заранее известные GraphQL operations наружу как Connect/HTTP JSON и генерирует OpenAPI. Для внутренних REST сервисов потребуется написать тонкие GraphQL subgraphs/adapters либо custom module.

JWT/JWKS, `@authenticated` и scopes есть из коробки. Relationship-aware SpiceDB check потребует custom Go module с request/subgraph hooks. Это acceptable только как ранний PEP: owner service повторяет authoritative check перед mutation.

Источники:

- [Cosmo ConnectRPC REST/OpenAPI exposure](https://cosmo-docs.wundergraph.com/connect-rpc/consume-via-rest-openapi)
- [Cosmo authentication and authorization](https://cosmo-docs.wundergraph.com/router/authentication-and-authorization)
- [Cosmo custom Go modules](https://cosmo-docs.wundergraph.com/router/custom-modules)

## 2. The Guild Hive Gateway

Hive Gateway — MIT-licensed Federation gateway, который запускается как standalone binary, Docker image или JavaScript package. Он может читать локальный supergraph, поэтому hosted Hive Registry не обязателен; сам Hive Console также имеет self-hosted deployment.

Источники:

- [Hive Gateway overview, installation and license](https://the-guild.dev/graphql/hive/docs/gateway)
- [Hive Gateway repository](https://github.com/graphql-hive/gateway)
- [Self-hosting Hive Console](https://the-guild.dev/graphql/hive/docs/schema-registry/self-hosting/get-started)

### REST/OpenAPI и SpiceDB

Это самый удобный migration path для существующих REST/OpenAPI contracts Gavel:

- GraphQL Mesh Compose умеет загрузить OpenAPI/Swagger и сгенерировать Federation-compatible subgraph/supergraph;
- SOFA plugin может при необходимости выставлять GraphQL operations наружу как REST и Swagger;
- custom plugins имеют async request/fetch lifecycle hooks;
- `@policy` умеет получать policy data из внешнего service, поэтому Authz facade над SpiceDB можно подключить без custom gateway binary.

Owner services всё равно должны повторять authoritative SpiceDB check для mutation: edge policy не владеет доменными данными и не должен быть единственной защитой.

Источники:

- [GraphQL Mesh: OpenAPI/REST to GraphQL/Federation](https://the-guild.dev/graphql/mesh/v1)
- [Hive custom plugins и SOFA REST exposure](https://the-guild.dev/graphql/hive/docs/gateway/other-features/custom-plugins)
- [Hive authentication, authorization и `@policy`](https://the-guild.dev/graphql/hive/docs/gateway/authorization-authentication)

### Realtime fit

Hive поддерживает federated subscriptions с client SSE/WebSocket и upstream SSE/WebSocket/HTTP Callback. Однако transport support не означает durable delivery. В актуальном Rust Hive Router документация прямо описывает bounded buffers и пропуск сообщений для отставшего consumer; это полезное backpressure поведение, но не замена replay из Auction history. Для Hive Gateway replay/coalescing также остаются внешним компонентом.

Источник: [Hive federated subscriptions](https://the-guild.dev/graphql/hive/docs/gateway/subscriptions).

Практический риск — JS/TS extension/runtime path на нашем профиле. Это не основание отвергать Hive, но при 20k read observations/s и hot-lot fan-out его надо сравнить с Cosmo тем же k6 сценарием, теми же CPU/memory limits и теми же mock subgraph delays.

## 3. Grafbase Gateway — исключён

По capability set Grafbase был сильным кандидатом: Rust data plane, Federation, air-gapped local SDL, WebSocket/SSE/multipart subscriptions, declarative REST resolver и Rust/Wasm extensions с network access для custom authorization.

Однако выбирать его для нового проекта нельзя. Официальный репозиторий сообщает, что The Guild приобрела Grafbase, hosted platform уже выключена, Gateway/CLI находятся в maintenance mode, а репозиторий объявлен к архивированию к концу мая 2026. Рекомендуемая самим проектом миграция — Hive Router или Hive Gateway.

Источники:

- [Grafbase acquisition, maintenance и archive notice](https://github.com/grafbase/grafbase#readme)
- [Grafbase Gateway MPL-2.0 license](https://github.com/grafbase/grafbase/blob/main/LICENSE)

## Применимость к Gavel

| Требование Gavel | Готовый router закрывает | Что остаётся нашей системой |
|---|---|---|
| Federation composition `LotScreen` | Cosmo/Hive строят query plan и объединяют subgraph responses | Схемы, ownership полей, устранение N+1 и budgets каждого subgraph |
| Текущая цена P99 < 200 мс | Один GraphQL field можно направить только в Auction | Индекс/хранилище Auction, cache policy и k6 proof |
| Остальные query/mutation P99 < 500 мс | Parsing/planning/routing/telemetry | Доменные инварианты, идемпотентность, timeout budget и degradation |
| 20k read observations/s | Оба масштабируются горизонтально; Cosmo compiled, Hive проще расширять | Нагрузочный профиль и capacity model; vendor benchmarks не принимаются как proof |
| Hot lot, большой fan-out | Cosmo Streams дедуплицирует одинаковые Kafka-topic triggers; Hive поддерживает long-lived subscriptions | Partitioning, slow consumers, terminal-event priority, business coalescing |
| Reconnect/replay | Transport reconnect есть | Cursor/stream version, replay window, gap detection и exact terminal delivery |
| SpiceDB | Есть hooks/policy extension points | Authoritative check в owner service; batching/cache/invalidation policy |
| Temporal | Не относится к router | Processes остаётся process layer и владеет durable workflows |
| Payment callbacks | Не относятся к Federation | Прямой provider ingress в Payments с signature/idempotency validation |

## Рекомендуемая целевая граница

```text
Browser
  -> Envoy Gateway (optional outer ingress: TLS, coarse limits, routing)
       -> Cosmo Router or Hive Gateway (public GraphQL/Federation)
            -> Catalog subgraph
            -> Auction subgraph
            -> Payments subgraph
            -> Analytics subgraph
            -> Authz facade -> SpiceDB       (early pre-check only)
            -> Realtime/Auction event source -> Redpanda

Processes -> Temporal                         (outside router)
Payment provider -> Payments callback ingress (outside GraphQL)
```

Envoy Gateway здесь не второй BFF: он завершает transport/security concerns, а выбранный Federation router остаётся единственной GraphQL composition точкой.

## Shortlist и рекомендация

### 1. Primary spike: Cosmo Router

Выбрать первым из-за Apache-2.0, полного self-hosting, compiled data plane и прямой Kafka integration, которая лучше всего совпадает с уже принятым Redpanda event backbone.

Не принимать в ADR до проверки двух серьёзных пробелов:

- relationship-aware authorization на старте subscription через SpiceDB custom module;
- replay после disconnect и сохранение terminal events при hot-lot overload.

### 2. Challenger: Hive Gateway

Держать вторым кандидатом. Он предпочтительнее Cosmo, если spike покажет, что стоимость REST/OpenAPI adapters и custom auth module выше, чем выигрыш Cosmo Streams. Mesh Compose и async `@policy` дают самый короткий путь от текущего OpenAPI/Authz дизайна к Federation.

### 3. Не выбирать

- Grafbase — объявлен к архивированию.
- Envoy Gateway/kgateway/Higress как единственный BFF — не выполняют Federation query composition.

## Обязательный comparative spike

Одинаковый test harness запускается для Cosmo и Hive:

1. Локальная schema composition без vendor SaaS/control plane.
2. `LotScreen` через Catalog + Auction + Analytics; P99 < 500 мс.
3. `currentPrice` только через Auction; 20k observations/s, P99 < 200 мс.
4. `placeBid` с idempotency key и authoritative result; P99 < 500 мс.
5. Hot lot: 1M bid events, burst fan-out, slow consumers и ограниченная память.
6. Disconnect/reconnect с cursor: нет gaps, terminal event не теряется, intermediate price updates можно coalesce.
7. Redpanda compatibility для Cosmo Kafka provider; partition reassignment и router restart.
8. SpiceDB: edge pre-check, batch/cache policy и обязательная повторная проверка в owner mutation service.
9. Router/subgraph failure, rolling schema update и subscription resubscribe.
10. CPU, RAM, connection count, Redpanda lag и P50/P95/P99 фиксируются k6/OTel; winner выбирается по нашим данным, не по vendor benchmark.

До результата spike корректное решение — заменить Proposed ADR «Apollo Router» на более общий выбор **self-hosted Federation router**, но не фиксировать конкретный продукт как Accepted.
