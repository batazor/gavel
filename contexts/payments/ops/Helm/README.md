# Payments operations

This directory is owned by the Payments project.

## Build

```bash
docker build \
  -f contexts/payments/ops/Dockerfile \
  -t ghcr.io/batazor/gavel-payments:dev \
  contexts/payments
```

`Dockerfile.ignore` documents the intended build context. Docker automatically uses the equivalent `Dockerfile.dockerignore` located next to this non-default Dockerfile.

## Install

The cluster-wide Prometheus Operator/Grafana stack must be installed first.

```bash
helm upgrade --install payments contexts/payments/ops/Helm \
  --namespace payments --create-namespace
```

Локальный профиль принадлежит сервису и хранится в `values-local.yaml`: он включает только guarded `sandbox` adapter и берёт control token из существующего Secret. Production values сохраняют Stripe и не монтируют local control route. Render assertions находятся в `tests/assert-render.sh` и запускаются единым gate 7.3.

The chart owns the Deployment, Service, ServiceMonitor, PrometheusRule and Grafana dashboard for this project. Dashboard UID: `gavel-payments`.

## Runtime endpoints

- `GET /healthz` — liveness;
- `GET /readyz` — readiness;
- `GET /metrics` — Prometheus bootstrap metrics.
- `POST /graphql` — owner GraphQL boundary;
- `:9090` — trusted Processes gRPC;
- `/local/sandbox/*` — только в local profile и только с control token.

Перед rollout chart запускает отдельный idempotent migration Job. Runtime не применяет схему сам; database/provider/fingerprint/sandbox-control secrets приходят только через `secretKeyRef`.

## gRPC telemetry contract

Server interceptors propagate W3C trace context and expose `gavel_rpc_server_requests_total` and `gavel_rpc_server_duration_seconds` with only `service`, `rpc_service`, `rpc_method` and `grpc_code`. Domain rejection is distinguishable from transport failure. Entity ids, user ids and idempotency keys are forbidden in metric labels.

## Alert runbook

1. For `Unavailable`, inspect ServiceMonitor targets, Deployment availability, pod events and readiness before restarting anything.
2. For `RestartStorm`, check exit reason/OOMKilled, resource saturation and the last rollout; stop or roll back a correlated rollout.
3. For `BootstrapMetricsMissing`, compare `/metrics` with the metric contract and verify that relabeling kept `job=payments`.
4. Resolve only after healthy targets and the 30-day availability recording rule resume; attach pod/log/trace evidence to the incident.
5. For `PaymentsInvalidCallbackSignatures`, compare provider secret rotation time and node clock skew; never paste signatures or raw callback bodies into tickets.
6. For `PaymentsOutboxLagHigh`, inspect Debezium/Redpanda health and the oldest unpublished row age before replaying delivery.
7. For `PaymentsReconciliationStalled`, inspect provider timeouts and stale attempt counts; reconciliation must not invent a payment outcome.
