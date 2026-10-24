# BFF operations

This directory is owned by the BFF project.

## Build

```bash
docker build \
  -f contexts/bff/ops/Dockerfile \
  -t ghcr.io/batazor/gavel-bff:dev \
  .
```

`Dockerfile.ignore` documents the intended build context. Docker automatically uses the equivalent `Dockerfile.dockerignore` located next to this non-default Dockerfile.

## Install

The cluster-wide Prometheus Operator/Grafana stack must be installed first.

```bash
helm upgrade --install bff contexts/bff/ops/Helm \
  --namespace bff --create-namespace
```

The chart owns the Deployment, Service, ServiceMonitor, PrometheusRule and Grafana dashboard for this project. Dashboard UID: `gavel-bff`.

Image build использует pinned `wgc` и компонует immutable execution config из service-owned SDL Catalog, Auction и Payments. Runtime image — pinned non-root Cosmo Router; Rust bootstrap не входит в delivery path.

## Router endpoints

- `POST /graphql` — единая GraphQL-граница;
- `GET /health/live` — liveness;
- `GET /health/ready` — readiness;
- `GET /metrics` — Router metrics.

Execution config собирается локально и не требует Cosmo control plane либо graph token. Router содержит только composition/query-planning concerns; resolvers, авторизация ресурсов, идемпотентность и доменные решения остаются у subgraphs-владельцев.

## Local JWT ingress

Только local profile может включить `localAuth.enabled`. В этом режиме Envoy sidecar принимает публичный порт, проверяет подпись JWT по JWKS, точные issuer и audience, затем передаёт исходный Bearer token и verified claim headers в Cosmo Router. Сам Router остаётся доступен лишь внутри pod.

```sh
helm template gavel-bff contexts/bff/ops/Helm \
  -f contexts/bff/fixtures/local-auth/local-values.yaml
```

Chart намеренно завершает render ошибкой, если `localAuth.enabled=true` при любом `deploymentProfile`, кроме `local`. Runtime negative check `deploy/kind/auth/check-jwt.sh` проверяет missing token, неверные audience/issuer и повреждённую подпись; он запускается только после поднятия MVP cluster.

## Alert runbook

1. For `Unavailable`, inspect ServiceMonitor targets, Deployment availability, pod events and readiness before restarting anything.
2. For `RestartStorm`, check exit reason/OOMKilled, resource saturation and the last rollout; stop or roll back a correlated rollout.
3. For `BootstrapMetricsMissing`, compare `/metrics` with the metric contract and verify that relabeling kept `job=bff`.
4. Resolve only after healthy targets and the 30-day availability recording rule resume; attach pod/log/trace evidence to the incident.
