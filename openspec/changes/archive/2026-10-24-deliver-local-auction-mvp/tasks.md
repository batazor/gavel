# Tasks

> Реализация выполняется крупными блоками. Тесты и проверки добавляются рядом с кодом владельца, но не запускаются после каждого изменения; полный прогон собран в разделе 7.

## 1. Контракты и локальная платформа (`deploy/`, shared contracts)

- [x] 1.1 Зафиксировать topic/envelope registry для Auction public, Auction restricted, Payments attempts и quarantine/DLQ, включая owner, schema version, partition key, ACL и compatibility policy; проверить, что registry не допускает candidate identity в public topic и доступен из `make help` через contract-check target.
- [x] 1.2 Подготовить additive protobuf/Federation/SpiceDB contract changes и стабильные Buf/BSR dependency pins, необходимые MVP consumers; добавить contract fixtures/checks и отложить их выполнение до 7.1.
- [x] 1.3 Создать `deploy/kind/` с pinned cluster config и platform manifests для PostgreSQL, Temporal, Redpanda, Kafka Connect/Debezium, SpiceDB, local JWT issuer/JWKS и Mailpit; проверить полноту manifests и local values по inventory обязательных workloads.
- [x] 1.4 Реализовать безопасный local secrets/bootstrap path, topic/ACL creation, connector registration и idempotent migration jobs без committed credentials; проверить повторяемость алгоритма статическим dry-run/render mode, сам cluster поднять в разделе 7.

## 2. Producer и owner services (`contexts/catalog`, `contexts/auction`, `contexts/payments`)

- [x] 2.1 Довести Catalog runtime/subgraph до owner-backed `LotWorkspace` полей и идемпотентной команды публикации seed-лота; добавить проверки resolvers и обновить Catalog README/Mermaid/`flow.c4`, выполнение проверок отложить до 7.2.
- [x] 2.2 Подключить Auction GraphQL query/mutation runtime, public и restricted outbox envelopes к утверждённому registry, сохранив transaction/idempotency/privacy invariants; добавить producer/subgraph tests и обновить Auction README/Mermaid/`flow.c4`, запуск отложить до 7.2.
- [x] 2.3 Добавить Payments deterministic sandbox adapter и local-only outcome control через существующий provider-neutral port без ветвления domain layer; добавить conformance tests для success, recovery и expiry, запуск отложить до 7.2.
- [x] 2.4 Подключить Payments attempt outbox и process RPC runtime к local profile, сохранив provider objects/recovery URL вне broker events; добавить contract/adapter tests и обновить Payments README/Mermaid/`flow.c4`, запуск отложить до 7.2.
- [x] 2.5 Обновить service-owned Helm charts Catalog/Auction/Payments для migrations, ports, probes, secrets и local overlays; добавить chart assertions, выполнение `helm lint/template` отложить до 7.3.
- [x] 2.6 Подключить Catalog `LotListed` и Payments instrument-readiness outbox к отдельным Redpanda routes и реальным restart-safe Auction consumers с commit после durable inbox/domain результата; добавить adapter/runtime tests и registry/ACL assertions.

## 3. Notify owner (`contexts/notify`)

- [x] 3.1 Реализовать Notify domain/application lifecycle `PENDING → SENT | FAILED`, canonical fingerprint, exact replay и payload-conflict semantics с PostgreSQL migrations; добавить unit/repository tests, запуск отложить до 7.2.
- [x] 3.2 Реализовать trusted `SendNotification` gRPC adapter, workload authorization и безопасный operator read для smoke; добавить gRPC/auth/idempotency tests и выполнить их только в 7.2.
- [x] 3.3 Реализовать versioned settlement templates и SMTP/Mailpit adapter с bounded retries и telemetry redaction; добавить renderer/adapter tests для recovery и winner messages, запуск отложить до 7.2.
- [x] 3.4 Обновить Notify Helm chart, README, use-case Mermaid и `flow.c4`, включая local SMTP/PostgreSQL values и readiness; добавить chart/doc checks для финального прогона 7.3.

## 4. Event ingress и workflow wiring (`contexts/processes`)

- [x] 4.1 Реализовать Redpanda consumer adapter поверх существующего broker-neutral ingress с manual offset commit после durable signal, bounded retry и restart-safe consumer group; добавить adapter tests для ack/retry/rebalance, запуск отложить до 7.2.
- [x] 4.2 Реализовать validation/routing для всех MVP envelopes, restricted route и sanitized quarantine/DLQ/replay command без выполнения owner commands для invalid records; добавить fixtures и negative/privacy tests, запуск отложить до 7.2.
- [x] 4.3 Подключить Auction/Payments routes к production `signal-with-start` gateway и реальным Auction/Payments/Notify owner RPC activities; добавить integration tests с повтором, перестановкой и потерянным response, запуск отложить до 7.2.
- [x] 4.4 Расширить Processes telemetry, readiness и Helm local values для broker/Temporal/RPC dependencies; обновить README, settlement Mermaid и `flow.c4`, а проверки выполнить в 7.3.

## 5. Публичный GraphQL/Web и локальная авторизация (`contexts/bff`, `contexts/authz`, `packages/web`)

- [x] 5.1 Заменить BFF runtime path на pinned Cosmo Router execution config из service-owned SDL, подключить Catalog/Auction/Payments subgraphs и сохранить Router свободным от бизнес-правил; добавить composition/operation fixtures, запуск отложить до 7.1.
- [x] 5.2 Реализовать local JWT issuer/JWKS integration и development-only identity switcher для seller/two bidders с проверкой issuer/audience/signature на ingress; добавить negative tests и production-profile guard, запуск отложить до 7.2.
- [x] 5.3 Реализовать идемпотентный Authz bootstrap через owner APIs для user/workload principals и SpiceDB relationships без authorization bypass; добавить seed/repeat/authz tests и обновить Authz README/Mermaid/`flow.c4`, запуск отложить до 7.2.
- [x] 5.4 Подключить Web `LotWorkspace` и `PlaceBid` к реальному BFF, добавить version-aware polling, terminal UI, typed mutation errors и явное разделение demo/connected режима; добавить component/client tests, запуск отложить до 7.4.
- [x] 5.5 Обновить BFF/Web Helm values, ingress/ports, operations docs и affected LikeC4 views; добавить render/operation assertions для финального прогона 7.3.

## 6. One-command assembly и эксплуатационный путь (`deploy/kind`, корневой `Makefile`)

- [x] 6.1 Реализовать `mvp-up`, `mvp-down`, `mvp-seed`, `mvp-ready`, `mvp-smoke` и `mvp-diagnostics` с видимыми описаниями в `make help`; проверить command inventory и безопасные explicit cluster/namespace targets без запуска полного suite.
- [x] 6.2 Реализовать идемпотентный seed через public/owner boundaries для seller, двух bidders, payment readiness и короткого опубликованного аукциона; добавить fixture assertions, исполняемые в 7.5.
- [x] 6.3 Реализовать readiness probes для GraphQL owner data, connector route, Temporal task queues, owner RPC и Mailpit, а также sanitized failure bundle; добавить forced-failure fixtures, исполняемые в 7.5.
- [x] 6.4 Реализовать три изолированных smoke-сценария: happy settlement, expiry первого кандидата с переходом к следующему и restart Processes без дублей; проверить, что scripts используют публичные/owner boundaries и будут запущены единым вызовом в 7.5.
- [x] 6.5 Документировать prerequisites, чистый запуск, identity switcher, sandbox outcomes, диагностику и teardown в корневом README/`deploy/kind/README.md`; обновить общую LikeC4 deployment/process view и проверить команды документации в финальном прогоне.
- [x] 6.6 Реализовать local image build/load и установку всех обязательных service-owned Helm releases Catalog/Auction/Payments/Authz/Processes/Notify/BFF/Web в `mvp-up`, сохранив идемпотентность повторного запуска и explicit cluster/namespace guards.

## 7. Единый финальный verification gate

- [x] 7.1 Запустить `make openspec-validate`, `make buf-check`, `make buf-breaking`, `make graphql-check` и `make authz-validate`; исправить все contract/spec failures и сохранить успешный результат.
- [x] 7.2 Запустить форматирование, lint, unit/integration и service checks для Catalog, Auction, Payments, Processes, Notify, Authz и BFF через discoverable Make targets; исправить все failures без ослабления тестов.
- [x] 7.3 Запустить все Helm/chart, image build, architecture/LikeC4 и observability checks для затронутых owners и `deploy/kind`; подтвердить отсутствие production credentials и development-only auth в production values.
- [x] 7.4 Запустить Web/UI typecheck, lint, component tests и production build; проверить реальный GraphQL mode, polling monotonicity, typed errors и terminal UI.
- [x] 7.5 На чистом cluster выполнить `make mvp-up`, повторный `make mvp-up`, `make mvp-ready`, `make mvp-seed` и единый `make mvp-smoke`; подтвердить три terminal сценария, отсутствие duplicate payments/notifications, полезную диагностику forced failure и безопасный `make mvp-down`.

## Workflow follow-up

- После review синхронизировать четыре capability specs с основными спецификациями и архивировать change.
- После локального MVP отдельным change спроектировать публичный staging/production edge, HA, realtime delivery и live provider credentials.
