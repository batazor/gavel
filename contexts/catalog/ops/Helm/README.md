# Catalog operations

This directory is owned by the Catalog project.

## Build

```bash
docker build \
  -f contexts/catalog/ops/Dockerfile \
  -t ghcr.io/batazor/gavel-catalog:dev \
  contexts/catalog
```

`Dockerfile.ignore` documents the intended build context. Docker automatically uses the equivalent `Dockerfile.dockerignore` located next to this non-default Dockerfile.

## Install

The cluster-wide Prometheus Operator/Grafana stack must be installed first.

```bash
helm upgrade --install catalog contexts/catalog/ops/Helm \
  --namespace catalog --create-namespace
```

The chart owns the Deployment, Service, ServiceMonitor, PrometheusRule and Grafana dashboard for this project. Dashboard UID: `gavel-catalog`.

## Runtime endpoints

- `GET /healthz` — liveness;
- `GET /readyz` — readiness;
- `GET /metrics` — bounded Catalog metrics;
- `POST /graphql` — Federation subgraph;
- `:9090` — internal `gavel.catalog.v1.CatalogService`.

Readiness появляется после подключения PostgreSQL, Authz и S3 bucket. Любой fatal supervised worker завершает runtime; Deployment перезапускает pod. Chart создаёт migration Job, но не создаёт `Secret`: database URL, object-storage endpoint/access/secret и HMAC keys приходят через external `secretKeyRef`.

## gRPC telemetry contract

Server interceptors propagate W3C trace context and expose `gavel_rpc_server_requests_total` and `gavel_rpc_server_duration_seconds` with only `service`, `rpc_service`, `rpc_method` and `grpc_code`. Domain rejection is distinguishable from transport failure. Entity ids, user ids and idempotency keys are forbidden in metric labels.

## Alert runbook

1. For `Unavailable`, inspect ServiceMonitor targets, Deployment availability, pod events and readiness before restarting anything.
2. For `RestartStorm`, check exit reason/OOMKilled, resource saturation and the last rollout; stop or roll back a correlated rollout.
3. Для `PendingMediaStuck` проверить S3 HEAD/delete, возраст PENDING/READY и lease owner; не удалять `ATTACHED` row вручную.
4. Для `OutboxBacklogHigh` проверить Debezium connector, Redpanda и unpublished rows; retention никогда не применяют к unpublished rows.
5. Для `ReconciliationBacklogHigh` проверить Authz и expired leases. Grant удаляется только после `lot_exists=false`.
6. `dependency_failure` разбирается по trace и dependency health без добавления endpoint, id или token в labels.
7. Закрывать инцидент после восстановления targets/backlogs и 30-day availability rule; приложить pod/log/trace evidence.
