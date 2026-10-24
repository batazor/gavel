# Notify

Provider-neutral bounded context доставки пользовательских уведомлений. Processes решает, **кого, когда и почему** уведомить; Notify решает, **как сформировать и доставить** сообщение, дедуплицирует команды и хранит историю доставки.

## ADR

- [0001 — Инициализация проекта](adr/0001-init.md)

## Interface

- [gRPC contract](proto/gavel/notify/v1/notify_service.proto) — внутренняя идемпотентная команда доставки от Processes; правила — в [init ADR](adr/0001-init.md).

## Компоненты

- **Notification API** — trusted gRPC-команда `SendNotification` только от Processes и read-only `GetNotification` для доверенного smoke/operator workload.
- **Notification aggregate** — состояния `PENDING → SENT | FAILED`, canonical fingerprint, exact replay и typed conflict при повторном id с другим payload.
- **PostgreSQL repository** — notification, bounded delivery attempts, terminal result и безопасный transactional outbox принадлежат Notify.
- **Template renderer** — owner-controlled ссылки `settlement_candidate.v1`, `settlement_candidate_failed.v1`, `payment_recovery_required.v1`, `winner_confirmed.v1`, `bidder_outbid.v1`.
- **SMTP adapter** — Mailpit-compatible доставка с ограниченными retry; адрес восстановления остаётся только в теле письма и не попадает в telemetry.

## Юзкейсы

- [settlement-notifications](use-cases/settlement-notifications/README.md) — кандидат, восстановление оплаты и подтверждённый победитель
- [outbid-notification](use-cases/outbid-notification/README.md) — уведомление участника после authoritative потери лидерства

## Operations

- HTTP `:8080/healthz`, `/readyz`, `/metrics`; readiness проверяет PostgreSQL и доступность SMTP.
- gRPC `:9090`: `NotifyService` и внутренний `NotifyOperatorService`; доступ ограничивается mesh principal и NetworkPolicy.
- Миграции выполняются `gavel-notify migrate` либо pre-install/pre-upgrade Helm job.
- [Docker image, Helm chart, Prometheus rules и Grafana dashboard](ops/Helm/README.md)

Локальный профиль использует `ops/Helm/values-local.yaml`; секрет `gavel-notify-runtime` с ключом `database-url` создаёт platform bootstrap и не хранится в Git.

Telemetry содержит только template reference и bounded outcome. Recipient, notification id, idempotency key, template data, recovery URL и SMTP payload запрещены в labels и логах.
