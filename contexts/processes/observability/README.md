# Processes observability contract

Эта директория описывает interface метрик process worker. Реализация exporter может меняться, но имена, labels, units и смысл series меняются только вместе с [ADR-0002](../adr/0002-workflow-observability.md).

## Instrumentation contract

| Metric | Type | Labels | Смысл |
|---|---|---|---|
| `temporal_workflow_task_schedule_to_start_latency_seconds` | SDK histogram | `namespace`, `task_queue` | Ожидание workflow task в очереди |
| `temporal_activity_schedule_to_start_latency_seconds` | SDK histogram | `namespace`, `task_queue` | Ожидание activity task в очереди |
| `temporal_long_request_latency_seconds` | SDK histogram | `namespace`, `operation` | Задержка long-poll worker → Temporal |
| `temporal_long_request_failure_total` | SDK counter | `namespace`, `operation`, optional `status_code` | Ошибки long-poll RPC |
| `temporal_num_pollers` | SDK gauge | `namespace`, `task_queue`, `poller_type` | Активные pollers |
| `temporal_worker_task_slots_available` | SDK gauge | `namespace`, `task_queue`, `worker_type` | Свободные execution slots; применимо к fixed-size supplier |
| `gavel_process_workflow_terminal_total` | custom counter | `workflow_type`, `outcome` | Терминальный результат workflow |
| `gavel_process_worker_ready` | custom gauge | — | Все обязательные dependencies готовы, consumer активен и оба worker poller-а запущены |
| `gavel_process_dependency_ready` | custom gauge | `dependency` | Bounded readiness для `temporal`, `broker`, `broker_consumer`, `auction_rpc`, `payments_rpc`, `notify_rpc` |
| `gavel_process_ingress_delivery_total` | custom counter | `owner`, `route`, `outcome` | Receive/retry/quarantine/commit broker delivery без event identity или payload labels |
| `gavel_process_activity_total` | custom counter | `activity_type`, `outcome` | Bounded owner-effect outcomes |
| `gavel_process_signal_total` | custom counter | `event_type`, `disposition` | Applied/duplicate/stale/gap signal outcomes |
| `gavel_process_overdue_auction_workflows` | custom gauge | `namespace`, `task_queue` | Не подтверждённые закрытия после `ends_at + 120s` |

Prometheus exporter добавляет `_total` counters и `_seconds_bucket/_sum/_count` histograms. Не добавлять entity IDs в labels: для конкретного workflow используйте Temporal UI, trace correlation и structured logs.

Overdue gauge вычисляет 30-секундный watchdog через Temporal Visibility по Search Attributes `GavelCloseDeadline` и `GavelCloseConfirmed`. Он работает вне детерминированного workflow/replay. При недоступной Visibility метрика не обновляется: `ProcessBusinessMetricsMissing` сообщает о слепой зоне вместо ложного значения `0`.

## Dashboard

Dashboard UID: `gavel-processes-temporal`. Он отвечает в таком порядке:

1. есть ли customer impact — overdue auctions и failure ratio;
2. живы ли Temporal и workers;
3. может ли worker получить задачу — schedule-to-start, pollers и slots;
4. исправна ли связь worker → Temporal — long request latency/errors;
5. хватает ли CPU/memory и нет ли restart storm.

Readiness panel отдельно показывает каждую обязательную зависимость. Broker probe проверяет metadata public, restricted, payments и quarantine topics; owner RPC probe проверяет готовность tonic channels. Эти probes не читают payload и не выполняют доменные команды.

Canonical JSON и rules лежат в [Processes Helm chart](../ops/Helm/). Compose монтирует те же service-owned файлы, поэтому локальная панель не расходится с production.

## Runbooks

### `ProcessWorkerUnavailable`

1. Проверить deployment replicas, readiness, restarts/OOMKilled и последние rollout events.
2. Проверить `temporal_num_pollers` по task queue и доступность Temporal frontend из pod.
3. Остановить rollout при корреляции с новой версией; восстановить минимум одну совместимую worker replica.
4. После восстановления убедиться, что schedule-to-start и overdue gauge возвращаются к baseline.

### `TemporalServiceUnavailable`

1. Найти `temporal_service`, у которого нет ни одной healthy replica; отличить отказ роли от потери всего кластера.
2. Проверить frontend/history/matching, Temporal Postgres quorum/latency и cluster DNS/network policy.
3. Не очищать task queues и workflow history. Восстановить service/DB quorum; workers сами продолжат durable work.
4. Проверить, что pollers переподключились и backlog/overdue начали уменьшаться.

### `AuctionWorkflowOverdue`

1. Это customer-impact page: найти overdue executions через Temporal visibility по workflow type и close deadline.
2. Сопоставить schedule-to-start, worker slots, long-request failures и Auction command responses.
3. Безопасно повторить только идемпотентную `CloseAuction(now)` activity/операторскую команду с audit trail; не менять `ends_at` и не завершать workflow вручную без incident commander.
4. Подтвердить terminal event и уведомление клиента; сохранить workflow/trace IDs в incident, не в metric labels.

### `WorkflowFailureRatioHigh`

1. Разбить по `workflow_type`, deployment version и failure class в logs/traces.
2. Исключить business rejections: они не должны попадать в `unexpected_failure`.
3. При новой регрессии остановить rollout; для детерминизма использовать versioning Temporal workflow code.
4. Перед retry проверить идемпотентность activities и состояние owner context.

### `TemporalWorkerLatencyHigh`

1. Сравнить long-request latency/failure с Temporal frontend CPU, DB latency и network telemetry.
2. Если long-request нормален, а schedule-to-start высокий — искать нехватку pollers/slots или CPU worker.
3. Если оба высоки — проверить mesh/network и Temporal matching/frontend capacity.

### `ProcessActivityFailuresHigh`

1. Разбить по bounded `activity_type` и `outcome`; entity IDs искать только в Temporal UI/trace.
2. Для transient outcome проверить owner endpoint и retry exhaustion. Для rejection проверить owner contract, не форсировать domain transition вручную.
3. Перед повтором убедиться, что используется исходный idempotency key.

### `ProcessSignalGapDetected`

1. Найти `event_type` с disposition `gap`, проверить lag/offset ingress и restricted topic ACL.
2. Не синтезировать пропущенный owner event в Processes; восстановить доставку из owner outbox.
3. После replay убедиться, что monotonic version восстановилась без changed redelivery.

### `ProcessIngressDeliveryFailures`

1. Разбить series только по bounded `owner`, `route` и `outcome`; не добавлять topic key, event id или payload в labels/logs.
2. Для `commit_failed` проверить group membership и broker authorization: workflow effect уже мог быть принят, поэтому следующий delivery должен сойтись через тот же event id.
3. Для `failed` проверить Temporal transport и quarantine producer; offset не продвигать вручную.

### `ProcessQuarantineGrowing`

1. Использовать sanitized quarantine record: source topic/partition/offset, opaque fingerprint и safe code.
2. Исправить schema/configuration cause, затем выполнить explicit replay через ту же validation/idempotency границу.
3. Не копировать restricted payload, candidate identity, amount или recovery URL в incident labels и команды.

### `ProcessDependencyNotReady`

1. По label `dependency` определить Temporal, broker/topic metadata, consumer loop либо конкретный owner RPC channel.
2. Сверить безопасную карту `GET /readyz`; она не содержит endpoints, credentials или entity ids.
3. Восстановить dependency. Не обходить readiness и не переключать consumer group/offset вручную.

## Проверка перед production

- `promtool check rules` для canonical ruleset.
- Synthetic workflow на отдельной task queue подтверждает start → timer → activity → completion и page при остановке workers.
- k6-профиль создаёт expected starts/signals; во время теста dashboard не теряет series и не создаёт high-cardinality labels.
- Drill: остановить одну worker replica (warning не page), все workers (page), одну Temporal replica (degraded), весь Temporal endpoint (page).
