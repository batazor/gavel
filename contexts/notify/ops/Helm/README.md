# Notify operations

This directory is owned by the Notify project.

## Build

```bash
docker build \
  -f contexts/notify/ops/Dockerfile \
  -t ghcr.io/batazor/gavel-notify:dev \
  .
```

`Dockerfile.ignore` documents the intended build context. Docker automatically uses the equivalent `Dockerfile.dockerignore` located next to this non-default Dockerfile.

## Install

The cluster-wide Prometheus Operator/Grafana stack must be installed first.

```bash
helm upgrade --install notify contexts/notify/ops/Helm \
  --namespace gavel --create-namespace \
  -f contexts/notify/ops/Helm/values-local.yaml
```

The chart owns the Deployment, Service, ServiceMonitor, PrometheusRule and Grafana dashboard for this project. Dashboard UID: `gavel-notify`.

До установки platform bootstrap создаёт Secret `gavel-notify-runtime` с ключом `database-url`. Chart не содержит credentials. Pre-install/pre-upgrade Job выполняет additive SQL migrations до readiness Deployment.

## Runtime endpoints

- `GET /healthz` — liveness;
- `GET /readyz` — PostgreSQL + SMTP readiness;
- `GET /metrics` — Prometheus metrics;
- `gavel.notify.v1.NotifyService/SendNotification` — Processes workload only;
- `gavel.notify.v1.NotifyOperatorService/GetNotification` — trusted smoke/operator workload only.

Local SMTP указывает на Mailpit `:1025`. Runtime никогда не логирует recipient, body, recovery URL, notification/idempotency id или template data. Operator result содержит только recipient, template/version, status, attempt count и aggregate version.

Проверка chart выполняется в финальном gate:

```bash
helm lint contexts/notify/ops/Helm
helm template notify contexts/notify/ops/Helm \
  --namespace gavel \
  -f contexts/notify/ops/Helm/values-local.yaml \
  | bash contexts/notify/ops/Helm/tests/assert-render.sh
```

## gRPC telemetry contract

Server interceptors propagate W3C trace context and expose `gavel_rpc_server_requests_total` and `gavel_rpc_server_duration_seconds` with only `service`, `rpc_service`, `rpc_method` and `grpc_code`. Domain rejection is distinguishable from transport failure. Entity ids, user ids and idempotency keys are forbidden in metric labels.

## Alert runbook

1. For `Unavailable`, inspect ServiceMonitor targets, Deployment availability, pod events and readiness before restarting anything.
2. For `RestartStorm`, check exit reason/OOMKilled, resource saturation and the last rollout; stop or roll back a correlated rollout.
3. For `BootstrapMetricsMissing`, compare `/metrics` with the metric contract and verify that relabeling kept `job=notify`.
4. Resolve only after healthy targets and the 30-day availability recording rule resume; attach pod/log/trace evidence to the incident.
