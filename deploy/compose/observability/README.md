# Local observability stack

Compose imports the same service-owned rules and Grafana dashboards used by every service Helm chart.

```bash
docker compose -f deploy/compose/observability/compose.yaml up -d
```

- Grafana: <http://localhost:3000>, default `admin/admin` (override `GRAFANA_ADMIN_USER` and `GRAFANA_ADMIN_PASSWORD`).
- Prometheus: <http://localhost:9090>.
- Alertmanager: <http://localhost:9093>; local config deliberately sends no external notifications.

Expected host metrics ports are `9101..9108` in project order Analytics, Auction, Authz, BFF, Catalog, Notify, Payments, Processes; Temporal remains `8000`. Example: `PORT=9102 cargo run -p gavel-auction`.

On macOS/Windows Compose reaches them through `host.docker.internal`. On Linux `extra_hosts: host-gateway` provides the same name. If local workloads run inside another Compose network, override the targets in `prometheus.yml`.

Stop containers without deleting local time series/dashboards:

```bash
docker compose -f deploy/compose/observability/compose.yaml down
```

Add `--volumes` only when you intentionally want to erase local observability data.
