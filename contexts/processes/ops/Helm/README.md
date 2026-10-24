# Processes operations

This directory is owned by the Processes project.

## Build

```bash
docker build \
  -f contexts/processes/ops/Dockerfile \
  -t ghcr.io/batazor/gavel-processes:dev \
  contexts/processes
```

`Dockerfile.ignore` documents the intended build context. Docker automatically uses the equivalent `Dockerfile.dockerignore` located next to this non-default Dockerfile.

## Install

The cluster-wide Prometheus Operator/Grafana stack must be installed first.

```bash
helm upgrade --install processes contexts/processes/ops/Helm \
  --namespace processes --create-namespace
```

Для local kind runtime используется тот же chart с owner-owned overlay:

```bash
helm upgrade --install gavel-processes contexts/processes/ops/Helm \
  --namespace gavel-mvp --create-namespace \
  -f contexts/processes/ops/Helm/values-local.yaml
```

Overlay ссылается на `gavel-redpanda-processes` и `gavel-processes-temporal`; credentials создаёт `deploy/kind` bootstrap, в Git они не хранятся.

The chart owns the Deployment, Service, process-worker and Temporal ServiceMonitors, PrometheusRule and Grafana dashboard for this project. Dashboard UID: `gavel-processes-temporal`.

## Runtime endpoints

- `GET /healthz` — liveness;
- `GET /readyz` — безопасный readiness snapshot для broker topics, consumer, Temporal, Auction/Payments/Notify RPC и worker pollers;
- `GET /metrics` — Prometheus metrics.

Readiness не выполняет owner commands и не раскрывает endpoints, credentials или event data. Pod остаётся live, но снимается из ready при потере любой обязательной границы.

## Owner gRPC telemetry contract

Activity client interceptors propagate W3C trace context and expose `gavel_rpc_client_requests_total` and `gavel_rpc_client_duration_seconds` with only `service`, `rpc_service`, `rpc_method` and `grpc_code`. Panels keep retryable transport failures separate from typed domain rejection. Entity ids, user ids and idempotency keys are forbidden in metric labels.

Broker telemetry использует только `owner`, `route` и bounded `outcome`. Candidate identity, amount, recovery URL, event id, partition key и payload запрещены в metrics/logs. Quarantine содержит только безопасные source coordinates и opaque hashes.

## Alert runbook

Processes uses the specialized [Temporal/workflow runbooks](../../observability/README.md), including worker loss, Temporal role failure, queue latency and overdue auction closure.
