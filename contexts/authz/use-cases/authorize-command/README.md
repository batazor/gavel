# Авторизовать пользовательскую команду

Контекст: Authz · Участники: BFF, сервис-владелец, SpiceDB · Диаграмма: `flow.c4` → `flow_authorize_command`

## Цель

Не допустить выполнение protected command без permission и при этом не смешать authorization с бизнес-валидацией.

## Сценарий

```mermaid
sequenceDiagram
    autonumber
    actor User as Пользователь
    participant Mesh as Istio
    participant Router as Cosmo Router
    participant Authz as authz-service
    participant Receipts as Authz receipts DB
    participant SpiceDB
    participant Owner as Owner service
    participant Domain

    User->>Mesh: Protected mutation + Bearer JWT
    Mesh->>Mesh: Verify JWT signature
    Mesh->>Router: Verified iss + sub
    Router->>Authz: RegisterPrincipal(iss, sub, idempotency key)
    Authz->>Receipts: Claim BFF command fingerprint
    Authz->>SpiceDB: TOUCH platform:gavel#member@user:u1_…
    SpiceDB-->>Authz: written_at revision
    Authz->>Receipts: Complete opaque result + revision
    Authz-->>Router: principal_id + written_at_token
    Router->>Authz: gRPC optional CheckPermission / BulkCheck
    Authz->>SpiceDB: Evaluate permission at_least_as_fresh registration revision
    SpiceDB-->>Authz: allow / deny + revision
    alt Edge pre-check denies
        Authz-->>Router: deny
        Router-->>User: 403 or privacy-preserving 404
    else Edge allows
        Router->>Owner: Command + verifiable principal
        Owner->>Authz: gRPC authoritative CheckPermission<br/>at_least_as_fresh required revision
        Authz->>SpiceDB: Evaluate at required revision
        alt Authz unavailable
            Authz-->>Owner: unavailable
            Owner-->>User: Retryable 503
        else Permission denied
            Authz-->>Owner: deny
            Owner-->>User: 403 or 404
        else Permission allowed
            Authz-->>Owner: allow
            Owner->>Domain: Evaluate eligibility and invariants
            Domain-->>Owner: Success or 409 / 422
            Owner-->>User: Typed result
        end
    end
```

## Изменение отношений

Owner заранее выделяет resource id и отправляет идемпотентную команду Authz relationship writer. Writer проверяет mTLS workload identity и допустимый resource type, вызывает SpiceDB `WriteRelationships` и возвращает ZedToken. Только после этого owner одной локальной транзакцией сохраняет ресурс, token и outbox; orphan tuple после failed commit безопасно удаляет reconciliation.

Перед SpiceDB Authz claim-ит durable receipt по caller и idempotency key. Exact replay не выполняет второй write; изменённый payload конфликтует; после истечения lease только тот же fingerprint может безопасно повторить TOUCH/delete. BFF владеет registration/`operation`, а Catalog/Auction/Payments/Notify — только своими resource types; Processes relationship writes запрещены.

Local bootstrap использует тот же `RegisterPrincipal` и receipt path для трёх development actors, но отдельную workload identity. Ей разрешён только фиксированный `platform:gavel#process_worker@service_account:processes`; это исключение не разрешает записывать owner resource tuples или administrator.

Для `auction:participant`, который можно определить только после принятия ставки, порядок обратный: domain commit, затем grant. Private read до завершения provisioning fail closed, а повтор команды по idempotency key завершает relationship write.
