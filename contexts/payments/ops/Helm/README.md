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

The chart owns the Deployment, Service, ServiceMonitor, PrometheusRule and Grafana dashboard for this project. Dashboard UID: `gavel-payments`.

## Bootstrap endpoints

- `GET /` — hello-world response;
- `GET /healthz` — liveness;
- `GET /readyz` — readiness;
- `GET /metrics` — Prometheus bootstrap metrics.

The bootstrap binary proves the build/deployment path only. Domain, GraphQL and integration interfaces remain defined by the project ADR and replace this shallow HTTP implementation incrementally.

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
