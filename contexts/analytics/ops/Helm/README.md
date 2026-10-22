# Analytics operations

This directory is owned by the Analytics project.

## Build

```bash
docker build \
  -f contexts/analytics/ops/Dockerfile \
  -t ghcr.io/batazor/gavel-analytics:dev \
  contexts/analytics
```

`Dockerfile.ignore` documents the intended build context. Docker automatically uses the equivalent `Dockerfile.dockerignore` located next to this non-default Dockerfile.

## Install

The cluster-wide Prometheus Operator/Grafana stack must be installed first.

```bash
helm upgrade --install analytics contexts/analytics/ops/Helm \
  --namespace analytics --create-namespace
```

The chart owns the Deployment, Service, ServiceMonitor, PrometheusRule and Grafana dashboard for this project. Dashboard UID: `gavel-analytics`.

## Bootstrap endpoints

- `GET /` — hello-world response;
- `GET /healthz` — liveness;
- `GET /readyz` — readiness;
- `GET /metrics` — Prometheus bootstrap metrics.

The bootstrap binary proves the build/deployment path only. Domain, GraphQL and integration interfaces remain defined by the project ADR and replace this shallow HTTP implementation incrementally.

## Alert runbook

1. For `Unavailable`, inspect ServiceMonitor targets, Deployment availability, pod events and readiness before restarting anything.
2. For `RestartStorm`, check exit reason/OOMKilled, resource saturation and the last rollout; stop or roll back a correlated rollout.
3. For `BootstrapMetricsMissing`, compare `/metrics` with the metric contract and verify that relabeling kept `job=analytics`.
4. Resolve only after healthy targets and the 30-day availability recording rule resume; attach pod/log/trace evidence to the incident.
