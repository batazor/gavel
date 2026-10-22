# Apollo Router as the Gavel BFF

Date: 2026-10-22

## Вопрос

Может ли готовый Apollo GraphOS Router заменить собственный Rust BFF Gavel, который сейчас определён как единственная публичная точка входа, компонует экран лота, переводит команды в вызовы доменных сервисов и отдаёт realtime через REST/SSE с `Last-Event-ID`?

## Краткий вывод

Apollo Router может заменить **GraphQL-часть edge orchestration**: принять GraphQL query/mutation/subscription, построить query plan, вызвать несколько subgraph/REST endpoints и собрать единый ответ. Это существенно уменьшает объём собственного кода для screen composition, маршрутизации, стандартной GraphQL-безопасности и telemetry.

Он не является drop-in заменой текущего BFF. Для перехода надо принять новый публичный контракт — federated GraphQL supergraph вместо OpenAPI REST. Текущий SSE-контракт, нативный browser `EventSource`, `Last-Event-ID`, replay по версии Auction stream и coalescing hot-lot событий Router сам не воспроизводит. Apollo описывает cursor replay как отдельный guaranteed-delivery pattern с собственными subscription broker, event store и Redis; это остаётся нашим компонентом.

Поэтому решение имеет смысл только как осознанный переход на GraphQL-first API. Если Gavel должен сохранить публичный REST/OpenAPI и SSE без изменения клиента, Apollo Router не заменяет BFF: он добавит второй публичный протокол и новый слой.

## Что именно умеет Router

Apollo называет Router execution engine для graph-based API orchestration: он запускается в нашей инфраструктуре перед GraphQL и REST APIs, планирует выборки и объединяет их в единый GraphQL graph. Request lifecycle ограничен GraphQL pipeline: parsing, validation, query planning, выполнение subgraph requests и сбор GraphQL response. Это готовая реализация тех частей BFF, которые являются декларативной композицией данных, но не движок бизнес-процессов.

Источники:

- [Apollo Router overview](https://www.apollographql.com/docs/graphos/routing)
- [Router request lifecycle](https://www.apollographql.com/docs/graphos/routing/request-lifecycle)

### Матрица применимости к Gavel

| Текущая обязанность BFF | Apollo Router | Что остаётся Gavel |
|---|---|---|
| Единая публичная точка входа | Да, для GraphQL operations | Ingress, payment provider callbacks в Payments |
| Композиция `LotScreen` | Да, через query plan по subgraphs/Connectors | Владение и качество каждого поля остаётся у доменных сервисов |
| Узкий read-path текущей цены | Да, если поле разрешается одним Auction subgraph fetch | SLO P99 < 200 мс, модель хранения и нагрузочный тест Auction |
| Перевод HTTP command в доменную команду | Частично: GraphQL mutation маршрутизируется в subgraph или REST Connector | Идемпотентность, инварианты и результат команды реализует сервис-владелец |
| Durable settlement/payment workflow | Нет | Temporal и Processes остаются без изменений |
| OpenAPI REST `/api/v1` | Нет как публичный contract | Либо миграция клиента на GraphQL, либо сохранение отдельного REST adapter |
| Public SSE + native `EventSource` | Нет | GraphQL subscriptions используют multipart HTTP; для SSE нужен отдельный gateway |
| `Last-Event-ID` replay и coalescing | Нет как встроенная гарантия | Subscription broker/event store, cursor semantics и hot-lot policy |
| JWT validation | Да в GraphOS Router, это лицензируемая GraphOS feature | Issuer/audience policy и identity propagation |
| SpiceDB authorization | Только через custom policy/coprocessor или в subgraphs | SpiceDB data, authoritative checks и ownership отношений |
| Telemetry, limits, traffic shaping | Да, значительная часть готова | Наши SLI/SLO, capacity validation и alerts |

## Supergraph — обязательная модель, а не опция

Router не принимает произвольную таблицу REST routes как обычный API gateway. В quickstart прямо требуется supergraph schema; она описывает GraphQL types, поля и subgraph, который каждое поле разрешает. Apollo Router Core также описан как router для federated supergraph на Apollo Federation 2.

Следовательно, выбор Apollo Router означает:

1. публичный contract браузера становится GraphQL;
2. Catalog, Auction, Payments, Analytics и при необходимости Processes представлены как GraphQL subgraphs или REST endpoints через Connectors;
3. supergraph schema и composition pipeline становятся отдельными release artifacts;
4. breaking-change policy переносится с OpenAPI paths/DTO на GraphQL schema и operations.

Источники:

- [Apollo Router quickstart: create a supergraph schema](https://www.apollographql.com/docs/graphos/routing/get-started)
- [Apollo Router Core repository](https://github.com/apollographql/router)

## REST через Apollo Connectors

Connectors позволяют описать существующий JSON-over-HTTP endpoint директивой `@connect`: method, URL, input и response mapping задаются в GraphQL schema, а Router вызывает REST API и преобразует ответ без handwritten resolver. Поддерживаются `GET`, `POST`, `PUT`, `PATCH`, `DELETE`; сервис всё равно должен выполнять собственную authentication/authorization.

Для Gavel это может быть переходным слоем: не обязательно сразу переписывать внутренние Rust services в GraphQL subgraphs. Но Connectors не превращают существующий OpenAPI файл в готовый публичный API автоматически — надо спроектировать GraphQL schema и mappings. Они также не подходят для всей текущей границы:

- `@connect` поддерживает `Query` и `Mutation`, но не `Subscription`;
- сложные transformations лучше оставлять resolver/service layer;
- Connectors ориентированы на JSON-over-HTTP, поэтому текущий streaming SSE endpoint не является их задачей;
- у Connector requests нет эквивалента subgraph hook для Rhai/coprocessor; router, supergraph и execution hooks работают, но granular per-source customization ограничена.

Текущая документация Connectors требует совместимые версии Router, Federation spec и GraphOS build pipeline; quickstart требует GraphOS account, graph ref и API key. Это не следует считать полностью автономной заменой собственного BFF без проверки коммерческой и операционной зависимости.

Источники:

- [Why use Apollo Connectors](https://www.apollographql.com/docs/graphos/connectors/why-connectors)
- [REST API requirements](https://www.apollographql.com/docs/graphos/connectors/getting-started/requirements)
- [Connector version requirements](https://www.apollographql.com/docs/graphos/connectors/getting-started/version-requirements)
- [Connector limitations](https://www.apollographql.com/docs/graphos/connectors/reference/limitations)
- [Connectors quickstart](https://www.apollographql.com/docs/graphos/connectors/getting-started)

## Realtime и replay

### Транспорт к браузеру меняется

GraphOS Router принимает client subscription по HTTP и возвращает события как `multipart/mixed`. Он не принимает прямое client WebSocket connection. Между Router и subscription subgraph могут использоваться WebSocket или HTTP callback protocols.

Это несовместимо с текущим решением «native `EventSource` без библиотеки». Клиенту понадобится GraphQL client/adapter с поддержкой multipart subscriptions либо отдельный SSE endpoint рядом с Router.

Источники:

- [GraphQL subscriptions overview](https://www.apollographql.com/docs/graphos/routing/operations/subscriptions/overview)
- [Multipart HTTP protocol](https://www.apollographql.com/docs/graphos/routing/operations/subscriptions/multipart-protocol)

### Replay не встроен в Router

Apollo отдельно документирует guaranteed-delivery architecture для cursor resumption. Она требует:

- передавать cursor в subscription field;
- реализовать subscription broker;
- хранить metadata/checkpoint в Redis;
- хранить события в persistent event store;
- делать historical backfill до переключения на live events;
- acknowledge broker message только после успешной доставки.

Router остаётся transport/query-plan узлом этой схемы. Таким образом, существующие Gavel semantics (`Last-Event-ID = Auction stream version`, replay из Auction, затем live Redpanda, coalescing промежуточной цены с сохранением terminal events) не исчезают. Их можно перенести в `Realtime`/subscription-broker component, но его всё равно надо проектировать, писать и нагрузочно тестировать.

Apollo также указывает, что после падения Router существующие subscriptions автоматически не мигрируют: client должен переподписаться, а broker/subgraph — восстановить delivery с checkpoint. Это близко к текущей модели Gavel, но не является функцией Router «из коробки».

Источники:

- [Guaranteed Delivery Pattern](https://www.apollographql.com/docs/graphos/routing/operations/subscriptions/guaranteed-delivery)
- [HTTP Callback Subscriptions Best Practices](https://www.apollographql.com/docs/graphos/routing/operations/subscriptions/http-callback-best-practices)
- [Subscription Broker Pattern](https://www.apollographql.com/docs/graphos/routing/operations/subscriptions/subscription-broker)

## Authentication и SpiceDB authorization

GraphOS Router умеет валидировать JWT/JWKS, проверять issuer/audience, извлекать claims и передавать их в request context. Директивы `@authenticated` и `@requiresScopes` подходят для authentication и статических OAuth scopes.

Relationship-based authorization Gavel нельзя выразить только scope. Для него подходит `@policy`: Router извлекает required policies, а Rhai script или external coprocessor должен их вычислить. Так как Rhai не делает network requests, вызов Authz/SpiceDB требует external coprocessor либо проверки в subgraph. Coprocessor — отдельный HTTP service и лицензируемая GraphOS feature; он возвращает раннее edge decision, но добавляет latency и failure mode.

Рекомендуемая граница для Gavel не меняется:

- Router/coprocessor может делать opportunistic pre-check и отбрасывать заведомо запрещённые fields/operations;
- сервис-владелец mutation повторяет authoritative SpiceDB check перед изменением состояния;
- идентичность строится из проверенных `iss + sub`, а не из доверенных client roles;
- capabilities с resource-dependent permissions лучше вычислять resolver/subgraph-ом, где доступны фактические entity identifiers и domain context.

Эта defense-in-depth модель совпадает с официальной рекомендацией Apollo сохранять service-layer controls наряду с router-layer authorization.

Источники:

- [JWT authentication](https://www.apollographql.com/docs/graphos/routing/security/jwt)
- [Authorization directives and `@policy`](https://www.apollographql.com/docs/graphos/routing/security/authorization)
- [Authorization overview](https://www.apollographql.com/docs/graphos/routing/security/authorization-overview)
- [Router customizations](https://www.apollographql.com/docs/graphos/routing/customization/overview)
- [External coprocessing](https://www.apollographql.com/docs/graphos/routing/customization/coprocessor)

## Customization choices

| Механизм | Network I/O | Лицензирование/ограничение | Подходящий случай Gavel |
|---|---:|---|---|
| YAML configuration | Нет | Core/stock Router, зависит от feature | CORS, headers, timeouts, traffic shaping, telemetry |
| Rhai | Нет | Доступен в Router Core и GraphOS Router | Нормализация headers/claims, короткие synchronous checks |
| External coprocessor | Да | GraphOS licensed feature | Вызов Authz/SpiceDB для edge pre-check |
| Native Rust plugin | Да | Custom build под ELv2; Apollo не рекомендует и не поддерживает troubleshooting | Только если stock mechanisms принципиально недостаточны |

Если ради замены собственного BFF приходится собирать custom Router binary с большим plugin, цель «не писать BFF самим» теряется. Для Gavel разумный предел custom logic в Router — конфигурация, небольшой Rhai и, при принятии GraphOS dependency, узкий authz coprocessor.

Источники:

- [Rhai customization](https://www.apollographql.com/docs/graphos/routing/customization/rhai)
- [Creating a custom Router binary](https://www.apollographql.com/docs/graphos/routing/customization/custom-binary)

## Self-hosting, GraphOS и лицензия

Router разворачивается в нашей инфраструктуре как binary, Docker image или Helm chart. Можно запустить Apollo Router Core с локальным supergraph file без managed schema delivery. Это сохраняет data plane внутри Gavel infrastructure.

Однако Router Core не равен GraphOS Router по feature set. Apollo перечисляет subscriptions, JWT authentication, authorization directives, external coprocessing, distributed caches и ряд security controls как licensed GraphOS features. Для них Router при старте получает license через Apollo Uplink; offline license заявлена для Enterprise и требует ротации. При истёкшей лицензии и оставшихся включёнными GraphOS features документация описывает grace period, затем hard outage. Поэтому license/Uplink должны стать явной частью availability и procurement решения, а не неявной библиотечной зависимостью.

Исходный код Router Core доступен под Elastic License 2.0, а не OSI open-source license. ELv2 разрешает внутреннее использование, модификацию и распространение с приложением, но запрещает предоставлять существенную функциональность Router как managed service, обходить license-key functionality и удалять notices. Для Gavel как auction product обычное внутреннее/self-hosted использование допустимо, но юридическая проверка лицензии всё равно нужна.

Источники:

- [Self-hosting Apollo Router](https://www.apollographql.com/docs/graphos/routing/self-hosted)
- [Licensed GraphOS Router features](https://www.apollographql.com/docs/graphos/routing/graphos-features)
- [GraphOS Router license and failure behavior](https://www.apollographql.com/docs/graphos/routing/license)
- [Apollo Uplink](https://www.apollographql.com/docs/graphos/routing/uplink)
- [Apollo Router Core ELv2 license](https://github.com/apollographql/router/blob/dev/LICENSE)
- [Apollo ELv2 explanation](https://www.apollographql.com/trust/licensing)

## Варианты для Gavel

### A. GraphQL-first GraphOS Router

```text
Browser
  -> GraphOS Router
       -> Catalog subgraph / Connector
       -> Auction subgraph
       -> Payments subgraph / Connector
       -> Analytics subgraph
       -> Realtime subscription subgraph -> broker -> Redpanda + replay source
       -> authz coprocessor -> Authz -> SpiceDB

Processes -> Temporal       (не через Router runtime)
Payment provider -> Payments callback ingress
```

Плюсы: Router действительно заменяет большую часть custom edge orchestration; единая typed schema; готовые query planning, observability и GraphQL controls.

Цена: смена публичного API и client stack; GraphOS commercial/control-plane dependency для production subscriptions/authn/authz; собственный realtime broker всё равно остаётся.

### B. Apollo Router Core для queries, отдельный realtime edge

Router Core получает локально composed supergraph; authentication остаётся на ingress/subgraphs, authoritative SpiceDB — в сервисах; текущий SSE gateway сохраняется отдельно.

Плюсы: меньше GraphOS runtime dependency, можно начать с read composition.

Цена: две публичные границы (`/graphql` и SSE), Router заменяет лишь часть BFF, больше собственной security/realtime инфраструктуры.

### C. Сохранить REST/OpenAPI

Не использовать Apollo Router как замену BFF. Если обязательны прежние URLs, `application/problem+json`, native `EventSource` и OpenAPI codegen, нужен REST-oriented gateway/BFF. Connectors могут спрятать эти REST services за GraphQL, но не сохраняют текущий public contract.

## Рекомендация

Вынести решение в ADR только после короткого spike, но проверять **вариант A**: self-hosted GraphOS Router как единственную публичную GraphQL точку входа, доменные сервисы как Federation subgraphs, Temporal вне Router, authoritative SpiceDB checks в owner services, отдельный Realtime subscription broker для replay/coalescing.

Spike должен доказать до смены accepted ADR:

1. `LotScreen` собирается из Catalog, Auction, Analytics и Payments одним GraphQL query без N+1 и укладывается в P99 500 мс.
2. `currentPrice` идёт только в Auction и укладывается в P99 200 мс при hot-lot profile.
3. `placeBid` сохраняет `Idempotency-Key` semantics и возвращает синхронный authoritative result.
4. SpiceDB pre-check через coprocessor не становится обязательной единственной проверкой и имеет fail-closed policy для protected mutations.
5. Subscription восстанавливается после разрыва по Auction stream version без потери terminal event; документирована замена `EventSource` на multipart GraphQL client.
6. Посчитана стоимость GraphOS при ожидаемых query/mutation requests и subscription update events; Free plan не рассматривается как production capacity.
7. Проверен режим отказа Uplink/license и выбран online license либо Enterprise offline license.

До прохождения spike не следует удалять текущие OpenAPI/SSE artifacts: выбор Apollo Router меняет публичный protocol, а не только реализацию контейнера.
