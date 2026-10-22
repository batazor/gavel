# 17. Prometheus Operator and Grafana observability platform

Date: 2026-10-22

## Status

Accepted

## Context

Все проекты экспортируют telemetry, но target discovery, rule evaluation, notification routing и dashboards не принадлежат одному bounded context. Нужен self-hosted production stack, управляемый тем же Helm/ArgoCD delivery path, и локальный способ импортировать те же dashboards и rules.

## Decision

- Kubernetes устанавливает `kube-prometheus-stack` и тем самым Prometheus Operator, Prometheus, Alertmanager, Grafana, kube-state-metrics и node exporter.
- Проекты владеют своими `ops/Helm`, `ServiceMonitor`, SLI, recording/alerting rules, dashboards и runbooks. Общий chart устанавливает только monitoring platform и её HA defaults; service-specific manifests в нём запрещены.
- Prometheus Operator выбирает targets через `ServiceMonitor`/`PodMonitor` и rules через `PrometheusRule`; ручные scrape configs для Kubernetes workloads не используются.
- Prometheus и Alertmanager запускаются минимум в двух replicas. Prometheus хранит 15 дней локально; Mimir остаётся долговременным backend через remote write после отдельной sizing/configuration работы.
- Две Grafana replicas используют внешний HA Postgres для общего состояния; SQLite и один ReadWriteOnce PVC не считаются production HA.
- Grafana dashboards provisioned as code. UI разрешён для исследования, но сохранённое изменение переносится обратно в JSON перед merge.
- Alertmanager отвечает за grouping, inhibition и routing. `page` идёт 24×7 on-call, `warning` — командному каналу, `ticket` — backlog; endpoint credentials живут только в Kubernetes Secrets.
- Docker Compose поднимает single-node Prometheus, Alertmanager и Grafana и монтирует те же project artifacts. Это development adapter, не production topology.

## Consequences

- Общая платформа остаётся системной, а dashboards/rules локализованы во всех проектах-владельцах; расширенное знание Temporal workflow остаётся в Processes.
- CRD versions и kube-prometheus-stack upgrades требуют явной проверки release notes и CRD migration.
- Локальный stack воспроизводит queries/dashboard/rules, но не HA, zone failure или production retention.

## References

- [Processes ADR-0002](../../contexts/processes/adr/0002-workflow-observability.md)
- [Observability chart](../../deploy/charts/observability/README.md)
- [kube-prometheus-stack](https://github.com/prometheus-community/helm-charts/tree/main/charts/kube-prometheus-stack)
- [Prometheus Operator design](https://prometheus-operator.dev/docs/getting-started/design/)
- [Prometheus alerting overview](https://prometheus.io/docs/alerting/latest/overview/)
- [Grafana provisioning](https://grafana.com/docs/grafana/latest/administration/provisioning/)
