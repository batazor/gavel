# Gavel local MVP

Этот каталог содержит pinned `kind` topology, локальные owner releases и платформу: PostgreSQL, Temporal, Redpanda, Kafka Connect/Debezium, SpiceDB, Redis, development-only JWT issuer/JWKS и Mailpit. Для медиа Catalog включает явным local-only флагом лёгкий in-process object-storage adapter; production S3 path остаётся неизменным. `inventory.yaml` является проверяемым перечнем workloads, generated Secrets и bootstrap stages.

## Предварительные требования и чистый запуск

Нужны Docker Desktop/Engine, `kind`, `kubectl`, Helm, `curl`, `jq` и OpenSSL. Выделенный kind создаётся без default CNI, после чего `scripts/platform.sh` последовательно устанавливает pinned Cilium, Istio control plane, cert-manager и Prometheus Operator. На внешнем Kubernetes эти компоненты остаются ответственностью платформы кластера. Service-owned charts применяют сетевые и workload-identity политики, `ServiceMonitor` и `PrometheusRule` также в local profile. Команды жёстко ограничены cluster `gavel-mvp`, context `kind-gavel-mvp` и namespace `gavel-mvp`.

```sh
make mvp-platform # kind + Cilium + Istio + cert-manager + Prometheus Operator
make mvp-up       # build/load 8 local images, platform bootstrap, Helm install, readiness
make mvp-up       # обязательный безопасный повтор
make mvp-ready    # graph, owner endpoints, connectors, workers и Mailpit
make mvp-seed     # seller, 2 bidders, READY instruments и короткий OPEN auction
make mvp-smoke    # happy, first-candidate expiry, Processes restart
make mvp-e2e      # Playwright: 2 bidders → SOLD → Temporal → Mailpit
```

`mvp-up` готовит cluster platform, собирает и загружает в kind `gavel-{catalog,auction,payments,authz,processes,notify,bff,web}:local`, затем устанавливает service-owned charts. Все крупные стадии выполняются последовательно. Первый запуск может занять заметное время из-за platform и Rust/container images.

Секреты не хранятся в Git. `scripts/secrets.sh` создаёт случайные local-only значения непосредственно в namespace `gavel-mvp` и сохраняет существующие значения при повторном запуске. Удаление выделенного cluster удаляет эти значения вместе с локальными данными.

Статическая проверка, не создающая cluster и не показывающая секреты:

```sh
deploy/kind/scripts/check.sh
deploy/kind/scripts/render.sh > /tmp/gavel-mvp.yaml
deploy/kind/scripts/bootstrap.sh --dry-run
```

Bootstrap ожидает context `kind-gavel-mvp`. Порядок фиксирован: local image build/load → generated Secrets → platform apply → PostgreSQL bootstrap → idempotent owner migrations/releases через Helm → Authz post-install bootstrap через owner gRPC API → JWT opaque-principal synchronization → Temporal namespace → registry-aligned topics/users/ACL → connector reconciliation через REST `PUT`.

Authz Job регистрирует `seller-local`, `bidder-one-local`, `bidder-two-local` у local issuer, получает opaque `u1_…` principals и создаёт только `platform:gavel#process_worker@service_account:processes`. Повторный Helm upgrade использует те же idempotency keys и не создаёт дубли. Job не получает SpiceDB credential; owner-specific lot/auction/payment tuples создаются последующим domain seed.

Local issuer публикует discovery/JWKS по `http://local-jwt:8080/default/.well-known/openid-configuration`. Mailpit принимает SMTP на `mailpit:1025`, а UI доступен на host port `8025` после запуска cluster.

## Identity switcher и sandbox

Web local profile использует три client id: `gavel-local-seller`, `gavel-local-bidder-one`, `gavel-local-bidder-two`. Authz bootstrap возвращает opaque `u1_…` IDs; `scripts/identity.sh` записывает их только в local JWT ConfigMap. Envoy проверяет issuer/audience/signature и передаёт owner subgraphs только verified principal headers и correlation id.

Payments local profile использует deterministic sandbox. Smoke задаёт outcome только через защищённый control boundary:

- `success` — первый кандидат оплачивает;
- `expire` — recovery attempt принудительно истекает через local-only owner control, затем выбирается следующий кандидат;
- `recovery_then_success` — доступен для ручной проверки continuation.

Control token хранится только в generated Secret `gavel-payments-runtime` и не выводится scripts.

Catalog local profile выдаёт одноразовый upload URL собственного HTTP adapter и проверяет те же MIME type, length и SHA-256 metadata, что production S3 boundary. Adapter хранит объекты только в памяти pod и предназначен исключительно для воспроизводимого kind MVP; restart Catalog очищает локальные бинарные fixtures.

## Доступ и диагностика

Для ручного UI/API доступа:

```sh
kubectl --context kind-gavel-mvp -n gavel-mvp port-forward service/gavel-web-gavel-web 4173:8080
kubectl --context kind-gavel-mvp -n gavel-mvp port-forward service/gavel-bff-gavel-bff 8080:8080
```

При ошибке `mvp-up` автоматически пытается собрать диагностику. Ручной вызов:

```sh
make mvp-diagnostics
```

Bundle в `.gavel-mvp/diagnostics/<UTC timestamp>/` содержит status/events/endpoints, connector inventory и последние container logs. Bearer credentials, passwords/tokens, recovery URL и maximum bid редактируются. Для teardown используйте `make mvp-down`; операция удаляет только выделенный kind cluster, после чего данные восстановить нельзя.
