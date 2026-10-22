# Notify

Provider-neutral bounded context доставки пользовательских уведомлений. Processes решает, **кого, когда и почему** уведомить; Notify решает, **как сформировать и доставить** сообщение, дедуплицирует команды и хранит историю доставки.

## ADR

- [0001 — Инициализация проекта](adr/0001-init.md)

## Interface

- [gRPC contract](proto/gavel/notify/v1/notify_service.proto) — внутренняя идемпотентная команда доставки от Processes; правила — в [init ADR](adr/0001-init.md).

## Компоненты

- **Notification API** — идемпотентная команда `SendNotification(notification_id, recipient, template, data)`.
- **Notification aggregate** — состояния `Pending → Sent | Failed`, логические retry и audit.
- **Template renderer** — версионированные шаблоны без settlement-правил.
- **Channel adapter** — сменный email-провайдер; другие каналы можно добавить позже.
- **Transactional outbox** — `NotificationSent`/`NotificationFailed`, публикуемые Debezium.

## Юзкейсы

- [settlement-notifications](use-cases/settlement-notifications/README.md) — кандидат, восстановление оплаты и подтверждённый победитель

## Operations

- [Hello-world build scaffold](src/main.rs)
- [Docker image, Helm chart, Prometheus rules и Grafana dashboard](ops/Helm/README.md)
