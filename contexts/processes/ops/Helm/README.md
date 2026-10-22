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

The chart owns the Deployment, Service, process-worker and Temporal ServiceMonitors, PrometheusRule and Grafana dashboard for this project. Dashboard UID: `gavel-processes-temporal`.

## Bootstrap endpoints

- `GET /` — hello-world response;
- `GET /healthz` — liveness;
- `GET /readyz` — readiness;
- `GET /metrics` — Prometheus bootstrap metrics.

The bootstrap binary proves the build/deployment path only. Domain, GraphQL and integration interfaces remain defined by the project ADR and replace this shallow HTTP implementation incrementally.

## Owner gRPC telemetry contract

Activity client interceptors propagate W3C trace context and expose `gavel_rpc_client_requests_total` and `gavel_rpc_client_duration_seconds` with only `service`, `rpc_service`, `rpc_method` and `grpc_code`. Panels keep retryable transport failures separate from typed domain rejection. Entity ids, user ids and idempotency keys are forbidden in metric labels.

## Alert runbook

Processes uses the specialized [Temporal/workflow runbooks](../../observability/README.md), including worker loss, Temporal role failure, queue latency and overdue auction closure.
