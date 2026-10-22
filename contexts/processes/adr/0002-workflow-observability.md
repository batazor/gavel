# 2. Наблюдаемость workflow и production paging

Date: 2026-10-22

## Status

Accepted as the initial production monitoring contract. Пороговые значения пересматриваются по нагрузочным тестам и production telemetry.

## Что

Processes публикует Temporal Rust SDK metrics и небольшой набор business SLI. Prometheus оценивает recording/alerting rules, а Grafana показывает единый dashboard `Gavel / Processes / Temporal workflows`.

Наблюдаемость разделена на четыре сигнала:

1. доступность Temporal и process workers;
2. связь worker → Temporal: long-poll latency и failures;
3. способность worker забирать задачи: schedule-to-start, pollers и slots;
4. бизнес-результат: неожиданные terminal failures и своевременное закрытие аукциона.

## Зачем

Живой pod ещё не означает, что аукцион завершится вовремя. Отдельно могут отказать Temporal frontend, сеть, pollers, worker capacity или сам workflow. Одного `up` либо CPU dashboard недостаточно, чтобы отличить эти режимы.

При этом долго живущий workflow сам по себе исправен. `AuctionLifecycle` законно ждёт deadline, а `SettleAuction` — двухчасовое recovery window. Поэтому возраст открытого workflow не является alert condition.

## Требования

### Метрики

- Process worker экспонирует `/metrics` на named HTTP port Helm Service (bootstrap `:8080`) с Temporal SDK prefix `temporal_`.
- Temporal server экспонирует собственные OSS service metrics на named port `metrics`.
- Service discovery сохраняет Kubernetes Service name в label `temporal_service`; availability требует хотя бы одну здоровую replica каждой обнаруженной Temporal роли, а не «любой живой Temporal pod».
- Все series имеют низкокардинальные labels `namespace`, `task_queue`, `workflow_type`, `operation`; `workflow_id`, `auction_id`, `user_id` в metrics запрещены и остаются только в traces/logs.
- Processes публикует `gavel_process_workflow_terminal_total{workflow_type,outcome}`. `outcome` ограничен: `success`, `unexpected_failure`, `timed_out`, `terminated`, `cancelled`.
- Processes публикует gauge `gavel_process_overdue_auction_workflows`: число `AuctionLifecycle`, для которых `ends_at + 120s` прошло, но идемпотентная команда `CloseAuction` ещё не была подтверждена.
- `AuctionLifecycle` обновляет low-cardinality Temporal Search Attributes `GavelCloseDeadline` и `GavelCloseConfirmed` при переносе deadline и подтверждении команды. Отдельный watchdog loop вне workflow replay раз в 30 секунд читает Temporal Visibility и вычисляет overdue gauge; сбой самого запроса делает series отсутствующей и вызывает telemetry alert, а не публикует ложный ноль.
- Custom terminal counter записывается completion observer один раз на фактический terminal transition и никогда не увеличивается во время replay workflow history.
- Business rejection доменной команды не увеличивает `unexpected_failure`: workflow обязан обработать его как ожидаемый результат.
- Traces и logs продолжают идти по OTLP. Prometheus pull используется для operational SLI и target health; одна метрика не экспортируется одновременно двумя путями.

### Начальные SLO

| SLI | Цель | Окно |
|---|---:|---:|
| Temporal + worker availability | `>= 99.95%` | 30 дней |
| Workflow task schedule-to-start P99 | `< 5s` | rolling 10m |
| Activity schedule-to-start P99 | `< 5s` | rolling 10m |
| Worker → Temporal long request P99 | `< 1s` | rolling 10m |
| Unexpected workflow terminal ratio | `< 0.1%` при достаточном трафике | rolling 30m |
| Auction close deadline | нет неподтверждённых закрытий позже `ends_at + 120s` | continuous |

`99.95%` — availability objective всей платформы, но у каждого journey свой error budget. Schedule-to-start и RPC latency — диагностические SLO: они объясняют риск deadline miss, но не подменяют бизнес-SLI закрытия.

### Paging policy

- `severity=page`: Temporal полностью недоступен, нет process workers, есть overdue auction, либо высокий массовый failure ratio. Уведомление идёт on-call 24×7.
- `severity=warning`: деградировала replica/capacity, превышены latency thresholds или worker slots исчерпаны. Уведомление идёт в командный канал в рабочее время.
- `severity=ticket`: долгий budget burn без немедленного customer impact.
- Alertmanager группирует по `alertname`, `cluster`, `namespace`, `service`; alert более высокого уровня ингибирует производные симптомы того же сервиса.
- У каждого page есть `runbook_url`, summary без идентификаторов пользователей и dashboard UID.

## Реализация

- Системная платформа устанавливается umbrella chart поверх `kube-prometheus-stack`: Prometheus Operator, Prometheus, Alertmanager, Grafana, kube-state-metrics и node exporter.
- `ServiceMonitor` выбирает Temporal и process-worker Services по labels, а `PrometheusRule` загружает canonical ruleset.
- Один version-controlled Grafana JSON используется и Helm sidecar, и file provisioning в Docker Compose. Изменения в UI не являются источником правды.
- Compose поднимает Prometheus, Alertmanager и Grafana для локальной проверки тех же queries/rules. Он не доказывает production HA.
- В production Prometheus и Alertmanager имеют минимум две replicas, persistent storage и anti-affinity. Remote-write/long-term retention выбирается отдельно; локальный retention — 15 дней.

## Последствия

- Для критического пути есть page по пользовательскому результату, а не только по инфраструктуре.
- Worker instrumentation становится частью interface проекта Processes и проверяется тестами метрик.
- Нужно поддерживать labels стабильными: их изменение ломает recording rules и dashboards.
- Начальные thresholds могут оказаться слишком строгими или мягкими; изменение порога требует обновить этот ADR и k6/production evidence, но не новый ADR.

## Референсы

- [Observability contract и runbooks](../observability/README.md)
- [Processes operations chart](../ops/Helm/README.md)
- [Общий observability platform chart](../../../deploy/charts/observability/README.md)
- [ADR-0017 — Prometheus Operator и Grafana](../../../docs/adr/0017-prometheus-operator-and-grafana.md)
- [Temporal SDK metrics reference](https://docs.temporal.io/references/sdk-metrics)
- [Temporal OSS service metrics](https://docs.temporal.io/references/service-metrics)
- [Prometheus Operator design](https://prometheus-operator.dev/docs/getting-started/design/)
- [Grafana provisioning](https://grafana.com/docs/grafana/latest/administration/provisioning/)
