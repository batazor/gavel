# Auction operations

This directory is owned by the Auction project.

## Build

```bash
docker build \
  -f contexts/auction/ops/Dockerfile \
  -t ghcr.io/batazor/gavel-auction:dev \
  contexts/auction
```

`Dockerfile.ignore` documents the intended build context. Docker automatically uses the equivalent `Dockerfile.dockerignore` located next to this non-default Dockerfile.

## Install

The cluster-wide Prometheus Operator/Grafana stack must be installed first.

```bash
helm upgrade --install auction contexts/auction/ops/Helm \
  --namespace auction --create-namespace
```

The chart owns the Deployment, Service, ServiceMonitor, PrometheusRule and Grafana dashboard for this project. Dashboard UID: `gavel-auction`.

## Runtime endpoints

- `GET /healthz` — liveness;
- `GET /readyz` — readiness; возвращает `503`, если supervisor или обязательная зависимость не готовы;
- `POST /graphql` — Auction Federation operations;
- `GET /metrics` — bounded-label Prometheus metrics.

Internal gRPC слушает отдельный `rpc.port`. Перед rollout chart запускает idempotent migration Job. `DATABASE_URL` и `COMMAND_FINGERPRINT_KEY_HEX` читаются только через `secretKeyRef`; chart не создаёт и не материализует Secret.

## Telemetry contract

Команды логируются структурированно только с `operation` и `outcome`. Метрики покрывают commands, retries, idempotency, stream replay, outbox/inbox, consumers и cache tiers. Entity ids, user ids, idempotency keys, payment ids, amounts и maximum запрещены в log fields и metric labels.

## Alert runbook

1. Для `AuctionUnavailable` проверить targets, `/readyz`, PostgreSQL/Authz/Redis connectivity и состояние supervisor до рестарта.
2. Для `AuctionCommandFailureRatioHigh` разбить `gavel_auction_commands_total` по bounded `operation/outcome`, затем сопоставить dependency failures и rollout.
3. Для `AuctionInboxPayloadConflict` сохранить quarantined metadata, сравнить producer fixture/schema version; payload с private данными не копировать в тикет.
4. Для `AuctionConsumerFailures` проверить broker lag, неизвестные schema versions и доступность inbox/projection transaction.
5. Для `AuctionRedisFailureRatioHigh` подтвердить PostgreSQL fallback и его нагрузку, затем проверить latency/circuit breaker Redis; cache не является источником истины.
6. Для `AuctionRestartStorm` проверить OOM/exit reason и откатить коррелирующий rollout.
7. Закрывать alert только после восстановления recording rules и приложить обезличенные metric/trace evidence.
