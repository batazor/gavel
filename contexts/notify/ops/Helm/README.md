# Notify operations

This directory is owned by the Notify project.

## Build

```bash
docker build \
  -f contexts/notify/ops/Dockerfile \
  -t ghcr.io/batazor/gavel-notify:dev \
  contexts/notify
```

`Dockerfile.ignore` documents the intended build context. Docker automatically uses the equivalent `Dockerfile.dockerignore` located next to this non-default Dockerfile.

## Install

The cluster-wide Prometheus Operator/Grafana stack must be installed first.

```bash
helm upgrade --install notify contexts/notify/ops/Helm \
  --namespace notify --create-namespace
```

The chart owns the Deployment, Service, ServiceMonitor, PrometheusRule and Grafana dashboard for this project. Dashboard UID: `gavel-notify`.

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
3. For `BootstrapMetricsMissing`, compare `/metrics` with the metric contract and verify that relabeling kept `job=notify`.
4. Resolve only after healthy targets and the 30-day availability recording rule resume; attach pod/log/trace evidence to the incident.
