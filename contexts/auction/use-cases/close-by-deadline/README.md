# Закрытие по дедлайну

Контекст: Auction · Актор: время (Temporal) · Диаграмма: `flow.c4` → `flow_timer_close`

## Цель
Закрыть приём ставок, когда наступил `ends_at`, и выбрать первого settlement candidate. Победитель подтверждается позже, после оплаты.

## Варианты политики
| Конфигурация | Поведение |
|---|---|
| `scheduled_end` | Закрытие в назначенное время |
| `inactivity` | Закрытие, если после последней ставки прошло окно без новых ставок |
| оба | `max` из двух: плановое время, но каждая ставка в окне продлевает |
| + `hard_deadline` | Потолок, дальше не продлевается |

Формула и правила — в [README контекста](../../README.md#closing-policy).

## Разделение ответственности
- **Агрегат** при каждой ставке пересчитывает `ends_at` и пишет `DeadlineExtended`. Решение о продлении — здесь.
- **Workflow `AuctionLifecycle`** (Temporal) держит durable-таймер на текущий `ends_at`, сбрасывает его по `DeadlineExtended` и `AuctionRescheduled`, по срабатыванию шлёт `CloseAuction(now)`. Решений не принимает.

## Основной сценарий

```mermaid
sequenceDiagram
    autonumber
    participant Broker as Redpanda
    participant Worker as Process worker
    participant Temporal
    participant Auction
    participant Authz
    participant Aggregate as Auction aggregate
    participant DB as auction-db
    participant Realtime
    participant Web

    Broker-->>Worker: AuctionOpened(ends_at=18:00)
    Worker->>Temporal: Start AuctionLifecycle
    Note over Worker,Temporal: Durable timer survives worker restart
    Web->>Auction: PlaceBid at 17:45
    Auction->>Aggregate: Apply bid and closing policy
    Aggregate-->>Auction: DeadlineExtended(18:45)
    Auction->>Broker: DeadlineExtended(18:45)
    Broker-->>Worker: DeadlineExtended(18:45)
    Worker->>Temporal: Reset timer to 18:45
    Broker-->>Realtime: DeadlineExtended(18:45)
    Realtime-->>Web: endsAt = 18:45
    Temporal-->>Worker: Timer fired at 18:45
    Worker->>Auction: gRPC CloseAuction(now=18:45, idempotencyKey)<br/>trusted workload identity + deadline
    Auction->>DB: Lookup idempotency key + fingerprint
    Auction->>Authz: Check service permission close
    Authz-->>Auction: Allowed
    Auction->>Aggregate: Validate now >= ends_at
    Aggregate-->>Auction: AuctionClosed + SettlementCandidateSelected
    Auction->>DB: Atomic commit: events + projection + command result<br/>public outbox + restricted process outbox, expected version N
    Auction->>Broker: Public AuctionClosed from gavel.auction.public.v1
    Auction->>Broker: Restricted SettlementCandidateSelected<br/>from gavel.auction.processes.v1
    Broker-->>Realtime: Public CLOSED
    Broker-->>Worker: Restricted SettlementCandidateSelected
    Realtime-->>Web: closed, payment pending
    Worker->>Temporal: Start settlement workflow
```

## Альтернативы и ошибки
- Дедлайн сдвинулся, пока команда летела → typed gRPC rejection `too_early`; workflow ждёт следующий authoritative `owner_event` (`DeadlineExtended`/`AuctionRescheduled`) и заменяет durable timer.
- Повтор того же `CloseAuction` с тем же idempotency key → сохранённый typed success без новых событий. Повтор после уже закрытого аукциона с новым key → typed terminal/domain outcome без повторного append.
- Неверная workload identity → `UNAUTHENTICATED`; Authz deny → `PERMISSION_DENIED`; timeout, dependency failure или исчерпанный OCC retry → retryable gRPC status.
- Ставок не было → `AuctionClosed`, `AuctionUnsold`.

`SettlementCandidateSelected(candidate, amount, round)` никогда не добавляется в публичный `AuctionPublicEvent`: он записывается в отдельный `auction_process_outbox` и доставляется только Processes по ACL-protected `gavel.auction.processes.v1`. Публичный `gavel.auction.public.v1` содержит только безопасный terminal статус.

## Важно про кандидата
Первым settlement candidate становится **лидер**, а не «сделавший последнюю ставку». Последняя ставка могла лишь поднять цену лидера. Winner появится только после успешной оплаты; при forfeiture Auction пересчитает следующего кандидата.

## Инварианты
5, 6, 7 из README контекста. Для политики с `inactivity`: в закрытом аукционе нет ставок в последнем окне.

## Runtime contract

Workflow type `AuctionLifecycle` работает на `auction-lifecycle-v1`. Ingress использует стабильный workflow ID и `signal-with-start` с `USE_EXISTING`; signal называется `owner_event`. Query `lifecycle_state` и search attributes содержат только phase/deadline/confirmation без auction или user ID. Activity `close_auction` имеет стабильный idempotency key и переживает restart worker-а.
