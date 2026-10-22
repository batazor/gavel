# История цены и дашборды

Контекст: Analytics · Акторы: участник (график), оператор (Grafana) · Диаграмма: `flow.c4` → `flow_analytics`

## Цель
Показать график цены лота за всё время и дашборды по торгам: средняя closing price по категориям, активность по часам, конверсия первого settlement candidate в подтверждённого Winner.

## Логика

```mermaid
sequenceDiagram
    autonumber
    participant Owner as Domain owner
    participant DB as Owner PostgreSQL
    participant CDC as Debezium
    participant Broker as Redpanda
    participant CH as ClickHouse
    participant Analytics
    participant Router as Cosmo Router
    participant Web
    participant Grafana

    Owner->>DB: Commit domain state + public outbox
    CDC->>DB: Capture outbox from WAL
    CDC->>Broker: Publish public events
    Broker-->>CH: Kafka engine consume
    CH->>CH: MaterializedView → MergeTree projections
    Web->>Router: Query lot price history
    Router->>Analytics: Price series by lot_id
    Analytics->>CH: Read price_changes
    CH-->>Analytics: Eventually consistent series
    Analytics-->>Router: Price history
    Router-->>Web: Lot + history<br/>(current price comes from Auction)
    Grafana->>CH: Read analytical aggregates
```

## Ограничения
- Отставание секунды. Для графика приемлемо, для цены — нет.
- Только вставки, обновлений нет. Переигрывание — пересоздать MV и прочитать топик с начала.

## Запросы, которые должны быть быстрыми
- Ряд цены по лоту.
- Средняя цена закрытия по категории за период.
- Доля аукционов, оплаченных первым кандидатом; среднее число settlement rounds до `WinnerConfirmed`.
