# 14. Availability, replication and shard-ready storage

Date: 2026-10-21

## Status

Accepted as the initial production availability architecture. Physical shard count and cross-region topology are selected from benchmarks and dependency SLOs before production launch.

## Context

Пользователи должны с высокой доступностью:

- получать список текущих аукционов;
- открывать детали аукциона;
- читать актуальную публичную цену;
- размещать ставки.

Целевой availability SLO — `99.95%`. Точный error budget равен:

- `4 h 22 m 48 s` за 365 дней;
- `21 m 36 s` за 30 дней.

«5 часов в год» допустимо как грубое объяснение, но соответствует примерно `99.943%`, поэтому для отчётности используется точное значение.

Replication повышает доступность и read capacity. Sharding повышает aggregate capacity и уменьшает blast radius, но усложняет маршрутизацию, rebalance, failover и эксплуатацию. Ни replication, ни sharding не ускоряют один hot auction: все его ставки всё равно требуют единого порядка.

## Decision

### Availability SLI

Каждая критичная операция имеет собственный monthly SLO `≥ 99.95%`; усреднение быстрой операции с недоступной запрещено.

| Journey | Interface |
|---|---|
| Список текущих аукционов | query `lots(first, after)` |
| Детали аукциона | query `lot(id)` |
| История ставок | field `Auction.bidHistory(first, after)` |
| Актуальная публичная цена | query `currentPrice(auctionId)` |
| Размещение ставки | mutation `placeBid(input, idempotencyKey)` |

- `5xx`, timeout, broken connection и неожиданный `429` под design load считаются unavailable.
- Ожидаемый доменный ответ (`403`, `409`, `422`) означает, что система доступна и приняла решение; он не является successful business outcome, но не считается outage.
- Authentication error из-за отсутствующего/истёкшего token исключается из SLI BFF; недоступность IdP при валидной пользовательской сессии учитывается отдельно в end-to-end journey.
- Planned maintenance входит в error budget: production deployment должен быть rolling и без общего downtime.
- Subscription измеряется отдельно: успешное подключение/reconnect и задержка доставки terminal events. Обычный reconnect сам по себе не является outage, если клиент восстанавливает актуальную версию в пределах SLO.

### Stateless compute

- `cloudflared` запускается минимум в двух replicas across zones; Istio ingress, Web, BFF и каждый stateless service — минимум в трёх replicas с topology spread/anti-affinity.
- PodDisruptionBudget и rolling deployment не допускают одновременного удаления всех healthy replicas.
- Readiness исключает instance до приёма трафика; graceful shutdown сначала прекращает новый трафик, затем завершает in-flight requests и subscriptions.
- HPA масштабирует replicas по saturation и latency, но не считается заменой capacity testing.

### PostgreSQL HA per context

Каждый bounded context сохраняет собственную БД. В production каждый физический Postgres shard — HA cluster:

- один fenced leader и минимум две standby replicas в разных availability zones;
- automatic failover без ручного переключения;
- критичный write подтверждается после durable write на leader и минимум одной synchronous standby;
- acknowledged bids и payment outcomes имеют `RPO = 0` при потере одного node/AZ;
- initial database failover objective — `RTO ≤ 60 s`;
- backups и point-in-time recovery обязательны, но не считаются HA и не заменяют replicas.

### Куда направляются чтения

| Данные | Read target | Причина |
|---|---|---|
| Auction command validation и `PlaceBid` | leader того же shard | Требуется актуальное состояние и один порядок записи |
| Query `currentPrice(auctionId)` | leader-owned operational projection | Асинхронная replica может вернуть устаревшую цену; P99 < 200 ms достигается узким read-path, а не ослаблением consistency |
| Auction history/replay | read replica с lag/version guard; fallback на leader | История read-heavy, но replay должен закончиться актуальной версией |
| Catalog list/details | read replicas | Описание read-heavy и не участвует в принятии ставки |
| Catalog read-your-writes | leader или causal/LSN routing | Продавец должен увидеть только что созданный/изменённый лот |
| Payments commands и provider callbacks | leader | Idempotency и единственный successful payment требуют актуального состояния |
| Payment status | replica только с lag/read-your-writes guard; иначе leader | Нельзя откатить UI к более старому payment state |
| Notify commands/status | leader; delivery history может читать replica с lag guard | Идемпотентность команды и terminal delivery state требуют актуального состояния |
| Analytics | replicated ClickHouse shards | Eventually consistent по определению |

Приложение не выбирает случайную replica. Context repository/router знает intent чтения (`strong`, `read_your_writes`, `stale_ok`) и проверяет replica lag/required LSN. При превышении lag budget чтение идёт на leader или явно отказывает; устаревший ответ не маскируется как актуальный.

### Shard-ready, не premature sharding

- `auction_id` — routing/shard key для Auction event stream, outbox и публичных auction events.
- `lot_id` — routing key Catalog. Query/search строятся отдельными read-моделями, а не scatter-gather по всем write shards на горячем пути.
- PaymentInstrument и PaymentAttempt имеют разные access patterns; физический shard key Payments выбирается локальным ADR после измерения registration/settlement traffic.
- Ни один доменный invariant не требует cross-shard transaction. Межконтекстная координация остаётся через events/Temporal.
- Interface context не раскрывает номер shard; routing и resharding остаются implementation detail.
- Production topology поддерживает `N` независимых shards, каждый со своим leader и replicas. Начальное `N` выбирается из storage growth, write IOPS, replica lag, vacuum/backup/failover time и результатов k6, а не фиксируется произвольно сейчас.
- Hot auction целиком остаётся на одном shard. Его throughput решают command serialization, cache/snapshots и backpressure, не scatter-write.

### Остальные stateful dependencies

- Redpanda: минимум три brokers, replication factor `3`, minimum in-sync replicas `2`; producer acknowledgement не допускает потерю подтверждённого outbox event при отказе одного broker.
- Kafka Connect/Debezium: минимум три distributed workers. Connector outage не блокирует ставки, но connector lag, outbox age и retained WAL имеют alert и disk budget; после восстановления публикация продолжается at-least-once.
- ClickHouse: replicated shards и quorum metadata; потеря одного replica не останавливает ingestion/read.
- Temporal: несколько server replicas и отдельный HA Postgres; process worker запускается в нескольких replicas, workflow state не хранится в worker pod.
- Authz service и SpiceDB: минимум по три stateless replicas across zones и отдельный HA Postgres datastore с leader + минимум двумя standby; schema migration выполняется expand/migrate/contract без остановки checks.
- OTel/Grafana outage не останавливает bids, но telemetry buffers имеют bounded policy и не создают backpressure в доменном пути.
- Cloudflare — внешняя synchronous dependency публичных journeys. Несколько `cloudflared` connectors across zones и автоматический reconnect убирают single pod/AZ failure, но outage Cloudflare edge учитывается в end-to-end error budget.

### Degraded mode

- При failover Auction leader новые ставки получают retryable failure; BFF никогда не отвечает ложным `201`. Повтор с тем же `Idempotency-Key` безопасен.
- Catalog details могут продолжить читаться с replicas во время failover leader.
- Если нельзя гарантировать актуальность цены, система явно отказывает price read вместо выдачи известного stale значения как актуального.
- BFF может показать read-only состояние, но не имитирует успешную ставку и не принимает её «в очередь» без durable acknowledgement Auction.
- При недоступности SpiceDB mutations и private fields fail closed с retryable GraphQL error; cached allow не используется после истечения bounded revision window. Публичные list/price/history/subscription fields продолжают работать без Authz.
- При потере одного Tunnel connector новые и существующие запросы используют оставшиеся connectors; WebSocket/subscription после разрыва переподписывается с `afterVersion`.
- При полной недоступности Cloudflare или Tunnel публичный путь недоступен; DNS-only/origin bypass не включается как аварийный fallback, иначе защита edge перестаёт быть обязательной.

### Multi-region

Multi-AZ — обязательная начальная topology. Active-active multi-region сейчас не принимается: global ordering одного hot auction, payment callbacks и data residency требуют отдельного trade-off.

До production dependency SLOs должны доказать, что single-region multi-AZ укладывается в end-to-end `99.95%`. Если error-budget composition этого не подтверждает, отдельный ADR выбирает warm-standby или active-passive region failover и задаёт regional RTO/RPO.

## Consequences

- Replication закладывается сразу во все production stateful systems.
- Не все reads отправляются на replicas: consistency intent важнее формального распределения нагрузки.
- Physical sharding не усложняет первый deployment без benchmark, но keys, repository interfaces и data model не блокируют его включение.
- Синхронная standby увеличивает write latency; это проверяется против `PlaceBid P99 < 500 ms` из ADR-0013.
- Read replica lag, failover duration, replication quorum и unavailable minutes становятся обязательными SLI/alerts.
- Backups решают disaster recovery, а replicas — availability; одно не заменяет другое.

## References

- [ADR-0013 — Workload and capacity model](0013-workload-and-capacity-model.md)
- [ADR-0004 — Service per bounded context](0004-service-per-bounded-context.md)
- [ADR-0005 — Transactional outbox](0005-transactional-outbox-for-integration.md)
- [ADR-0012 — Redpanda](0012-redpanda-as-message-broker.md)
- [Auction init](../../contexts/auction/adr/0001-init.md)
- [BFF init](../../contexts/bff/adr/0001-init.md)
- [Processes init](../../contexts/processes/adr/0001-init.md)
- [Authz init](../../contexts/authz/adr/0001-init.md)
- [ADR-0016 — Cloudflare public edge](0016-cloudflare-as-public-edge.md)
