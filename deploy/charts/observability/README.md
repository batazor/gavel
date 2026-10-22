# Gavel observability chart

Umbrella chart устанавливает только общую monitoring platform: `kube-prometheus-stack`, Prometheus Operator, Prometheus, Alertmanager и Grafana. Текущая dependency `92.7.0` включает Prometheus Operator `v0.94.1`; major upgrades делаются отдельно из-за CRD migrations.

## Установка

```bash
helm repo add prometheus-community https://prometheus-community.github.io/helm-charts --force-update
helm dependency build deploy/charts/observability
helm upgrade --install gavel-observability deploy/charts/observability \
  --namespace observability --create-namespace
```

Production overlay требует заранее созданные Secrets:

```bash
helm upgrade --install gavel-observability deploy/charts/observability \
  --namespace observability --create-namespace \
  -f deploy/charts/observability/values-production.yaml
```

`grafana-database` задаёт `GF_DATABASE_TYPE=postgres`, `GF_DATABASE_HOST`, `GF_DATABASE_NAME`, `GF_DATABASE_USER` и `GF_DATABASE_PASSWORD`; `grafana-admin` задаёт admin credentials. Две Grafana replicas намеренно не используют локальный SQLite/PVC.

Каждый сервис сам устанавливает свои `ServiceMonitor`, `PrometheusRule` и dashboard из `contexts/<service>/ops/Helm`. Prometheus и Grafana настроены искать эти ресурсы во всех namespaces; общая платформа не знает имён project metrics или SLO.

Compose также читает rules/dashboard непосредственно из service-owned каталогов — копий в `deploy/` нет.
