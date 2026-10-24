# Design

## Context

См. `proposal.md` — Why. Доменные ядра и owner RPC для Auction, Payments и Processes уже существуют, как и service-owned Helm charts и проверяемые GraphQL operations. При этом корневые `kind-up/kind-down` остаются TODO, `contexts/bff/src/main.rs` и `contexts/notify/src/main.rs` являются bootstrap HTTP-серверами, а `contexts/processes/src/ingress/` содержит broker-neutral boundary без production Redpanda consumer. Из локальной платформы собран только отдельный observability compose.

Архитектурные ограничения:

- Catalog, Auction, Payments, Notify и Authz сохраняют собственные данные и contracts; Processes только оркестрирует.
- Браузер обращается только к Cosmo Router, а synchronous process commands идут только через versioned gRPC.
- Transactional outbox доставляется at-least-once; workflow и owner commands обязаны выдерживать повтор.
- Restricted settlement events не смешиваются с публичными topics.
- MVP должен работать без production credentials и внешних SaaS, но не вводить несовместимый с production путь данных.

## Goals / Non-Goals

**Goals:**

- Получить один воспроизводимый локальный `kind` runtime и один реальный вертикальный аукционный путь.
- Использовать те же Federation, protobuf, outbox и Temporal boundaries, которые предполагаются production архитектурой.
- Сохранить domain modules независимыми от Kafka, SMTP, Kubernetes и sandbox provider adapters.
- Сделать повторы, restart, quarantine и диагностические данные частью MVP acceptance, а не ручной демонстрацией.
- Выполнить реализацию крупными блоками и запустить полный набор проверок после сборки вертикального среза.

**Non-Goals:**

- Production ingress/edge, HA, autoscaling, disaster recovery и live provider credentials.
- Realtime subscription delivery, Analytics/ClickHouse и production observability deployment.
- Полноценный OAuth login UX; локальный identity switcher существует только в development runtime.
- Замена service-owned contracts или перенос бизнес-правил в Router, scripts либо Temporal workflows.

## Decisions

### 1. Локальная платформа живёт в `deploy/kind/` и собирается service-owned Helm charts

`deploy/kind/` будет владеть cluster config, platform dependencies, local values overlays, локальной сборкой и загрузкой owner images, установкой всех обязательных service-owned Helm releases, bootstrap, readiness и smoke orchestration. Корневой `Makefile` получит `mvp-up`, `mvp-down`, `mvp-seed`, `mvp-ready`, `mvp-smoke` и `mvp-diagnostics`; текущие `kind-up/kind-down` станут совместимыми aliases либо будут использовать те же scripts.

Service deployments продолжают собираться из `contexts/<owner>/ops/Helm`; `deploy/kind/values/` меняет только local configuration. Databases остаются логически service-owned даже если локально используют один PostgreSQL instance с раздельными databases/users. Миграции выполняются отдельными idempotent jobs до readiness приложений.

Внешний целевой Kubernetes считается оснащённым Cilium-compatible enforcement для `NetworkPolicy`, Istio security CRD/controller, Prometheus Operator и cert-manager. Поэтому local overlays не отключают service-owned ingress/workload-identity policies, `ServiceMonitor` или `PrometheusRule`; установка и lifecycle этих controllers принадлежат платформе кластера. Выделенный disposable kind является исключением для воспроизводимой локальной проверки: `deploy/kind/scripts/platform.sh` создаёт его без default CNI и последовательно устанавливает pinned Cilium, Istio control plane, cert-manager и Prometheus Operator. Полный production observability stack при этом не является частью Gavel MVP.

Для media fixtures kind не поднимает отдельный S3 workload: local values явно включают ephemeral in-process Catalog adapter, реализующий тот же object-storage port и проверки MIME type/length/checksum. Production default остаётся S3 SigV4 adapter, а реальная S3 совместимость покрывается отдельным disposable `s3s-fs` boundary test с pinned image digest. Это уменьшает нагрузку локального runtime без изменения GraphQL/domain contracts.

Почему не общий Docker Compose: принятый delivery boundary — kind/Helm, и Compose создал бы второй неподдерживаемый способ wiring. Существующий observability compose остаётся самостоятельным и не является prerequisite MVP.

### 2. Event delivery использует Debezium/Kafka Connect и Redpanda, а не прямой polling Processes

`deploy/kind/platform/` поднимает Redpanda и Kafka Connect/Debezium. Owner-specific connector configuration читает только разрешённые outbox tables Catalog, Auction и Payments и публикует versioned envelopes в отдельные topics:

- Catalog public lifecycle events для Auction;
- Payments instrument-readiness events для Auction;
- Auction public lifecycle events;
- Auction restricted process events;
- Payments attempt events;
- quarantine/DLQ для постоянных ingress failures.

`contexts/auction` запускает отдельные restart-safe broker consumers для Catalog `LotListed` и Payments `PaymentInstrumentReady`/`PaymentInstrumentRevoked`. Они вызывают существующие `CatalogEventConsumer` и `PaymentsEventConsumer`, подтверждают offset только после сохранённого inbox/domain результата и используют отдельную consumer group от Processes. Эти routes не проходят через Processes, потому что открытие Auction и payment-readiness projection принадлежат Auction как owner своих правил допуска.

`contexts/processes/src/adapters/inbound/` реализует Redpanda consumer поверх существующего `ingress` port. Consumer использует отдельную group, manual offset commit после успешного `signal-with-start`, bounded exponential backoff и явную классификацию transient/permanent failures. Envelope сохраняет owner event id/version; payload не преобразуется в новый источник истины.

Restricted topic получает отдельный principal/ACL только для Processes. Connector secrets и broker credentials создаются local bootstrap и не попадают в Git. Topic/connector readiness проверяется контрольной записью, не только открытым TCP port.

Почему не временный HTTP relay: он ускорил бы demo, но обошёл бы уже принятый outbox contract и не проверил бы restart/replay, являющиеся главным интеграционным риском.

### 3. Processes подтверждает record только после durable workflow acceptance

Broker adapter вызывает существующий version/order-aware gateway. Offset фиксируется после принятого Temporal signal либо доказанного duplicate/stale результата. Temporal transport failure оставляет record непотверждённым. Permanent validation/routing failure после bounded attempts записывает sanitized quarantine envelope и позволяет продолжить partition согласно документированной policy.

Replay выполняется отдельной operator command по исходному topic/partition/offset или quarantine id и проходит ту же validation/idempotency границу. Метрики используют только owner, event type, route и bounded outcome; event payload, candidate id, amount и recovery URL запрещены в labels/logs.

### 4. BFF deployment использует Cosmo Router, а Rust bootstrap не становится custom gateway

`contexts/bff/ops/Helm` запускает pinned Cosmo Router image с execution config, собранным существующими scripts/composition checks. `contexts/bff/operations/` остаётся источником client operations. Router передаёт verified principal и correlation context owner subgraphs, но не содержит resolver/domain code.

Catalog, Auction и Payments service binaries экспонируют свои существующие GraphQL adapters как subgraphs. Для MVP graph включает только types/fields, необходимые `LotWorkspace` и `PlaceBid`; subscription contract остаётся в schema, но не входит в readiness. Web в `packages/web/src/graphql/` использует query/mutation и version-aware polling, а demo fallback остаётся явно маркированным.

Почему не расширять `contexts/bff/src/main.rs`: собственный proxy дублировал бы Federation composition и нарушил бы ADR о единой Cosmo границе. Bootstrap binary можно удалить из runtime path либо оставить только как неиспользуемый scaffold до отдельной уборки.

### 5. Local auth использует development-only issuer и реальные Authz decisions

`deploy/kind/auth/` поднимает local-only JWT issuer/JWKS с тремя заранее известными users. Development identity switcher Web получает короткоживущий token у этого issuer и прикладывает его к BFF request; local ingress проверяет issuer/audience/signature до передачи claims. Этот path включается только local values и не собирается как production login.

`mvp-seed` регистрирует principals и записывает SpiceDB relationships через owner Authz APIs: seller владеет lot, два bidders могут участвовать, Processes workload получает только process permissions. Service-to-service identity передаётся отдельной workload configuration, а user id из payload не считается доказательством.

Почему не bypass authorization: обход сделал бы UI demo зелёным, не проверяя главный cross-context invariant. Почему не полноценный внешний OAuth: он добавляет внешнюю доступность и UX, не нужные для локального MVP.

### 6. Payments получает отдельный детерминированный sandbox adapter

Новый adapter в `contexts/payments/src/adapters/outbound/` реализует существующий provider-neutral port и выбирается только конфигурацией local values. Outcome задаётся fixture/control API, защищённым local-only network policy, либо детерминированным seed profile: success, recovery-then-success, expire. Domain/application layer не ветвится по sandbox/Stripe и продолжает использовать прежние idempotency/provider keys.

Provider callback path для sandbox генерирует тот же нейтральный application input, что и проверенный Stripe callback. `contexts/payments/src/main.rs` выбирает adapter при bootstrap; production default и Stripe contracts не меняются.

### 7. Notify становится полноценным owner service с PostgreSQL и Mailpit adapter

`contexts/notify/` получает application/domain modules, PostgreSQL migrations, tonic gRPC inbound adapter и outbound email port. Transaction сохраняет canonical fingerprint, state/version, attempt history и terminal result по notification id. Exact retry воспроизводит result; payload mismatch возвращает typed conflict.

Local values подключают SMTP adapter к Mailpit из `deploy/kind/platform/`. Template files/version metadata принадлежат Notify. Recovery URL разрешён только в message body соответствующего recipient и редактируется из telemetry. Processes вызывает только versioned Notify gRPC; delivery failure наблюдается, но не меняет workflow deadline или authoritative owner state.

### 8. Seed и smoke используют публичные и owner boundaries

`deploy/kind/scripts/seed` создаёт fixtures идемпотентными public/owner commands, а не прямой записью доменных таблиц. Допустимое исключение — platform bootstrap credentials и создание пустых databases/topics. Короткий deadline настраивается входными данными аукциона, а не ускорением production workflow semantics.

`deploy/kind/tests/mvp-smoke` работает через публичный GraphQL endpoint и sandbox control boundary, затем проверяет authoritative public/owner reads, Temporal status и Notify history. Он не читает private Auction events или provider objects. Три сценария запускаются в изолированных fixture namespaces/ids:

1. первый candidate оплачивает успешно;
2. первый round истекает, второй candidate побеждает;
3. Processes restart между event acceptance и terminal outcome не создаёт дубликатов.

Проверки кода и contracts (`fmt`, `lint`, unit/integration, Buf, GraphQL composition, Helm, LikeC4, OpenSpec) выполняются единым финальным verification этапом после реализации крупных блоков.

### 9. Observability и failure diagnostics входят в local acceptance

Каждый обязательный boundary имеет readiness probe и bounded metrics. `mvp-diagnostics` собирает Kubernetes events/status, sanitized recent logs, connector state/lag, topic offsets, Temporal workflow id/status и quarantine summary в локальный ignored каталог. Secrets, JWT, full event payload, bid maximum и recovery URL фильтруются.

Readiness отделена от liveness: запущенный pod не считается готовым, если migrations, owner dependencies, graph composition, connector route или Temporal namespace/task queue недоступны.

## Risks / Trade-offs

- **[Большой вертикальный change затрагивает много owners]** → Контракты и platform wiring реализуются раньше consumers; задачи сгруппированы по владельцам, а финальная проверка запускается после интеграции.
- **[Kafka Connect/Debezium усложняет локальный запуск]** → Пиновать images/config versions, выделить readiness probes и поддерживать один kind path вместо параллельного relay.
- **[Local issuer может случайно попасть в production]** → Отдельные namespace/values, явный `developmentOnly` guard и startup refusal при production profile.
- **[Polling создаёт лишнюю нагрузку]** → Bounded interval, pause для скрытой вкладки и остановка после terminal state; realtime остаётся последующим улучшением.
- **[Sandbox provider расходится с Stripe]** → Он реализует тот же application port и neutral outcomes; provider-specific conformance остаётся в Payments tests.
- **[At-least-once создаёт повторные сигналы]** → Event identity/version, manual commits, workflow deduplication и owner idempotency проверяются restart smoke.
- **[Слабая диагностика сделает one-command runtime хрупким]** → Failure автоматически вызывает sanitized diagnostics и указывает конкретную границу readiness.

## Migration Plan

1. Добавить local platform manifests/scripts и contract configuration, не переключая существующие deployments.
2. Реализовать owner adapters и migrations в Auction/Payments/Notify, затем Processes consumer.
3. Подключить Cosmo Router, subgraphs, local issuer/Authz seed и Web polling.
4. Поднять чистый cluster, применить migrations/connectors/topics/ACL и выполнить seed.
5. Запустить единый финальный verification и три smoke-сценария; только после этого считать local profile готовым.

Rollback удаляет выделенный kind cluster и local generated state; production values, contracts и deployments не переключаются. Service code rollback выполняется обычным Git revert вместе с его additive migrations/configuration, при этом уже добавленные nullable/additive database objects могут оставаться неиспользуемыми.
