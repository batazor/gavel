# Локальный Authz bootstrap

Контекст: Authz · Инициатор: Helm post-install/post-upgrade Job · Диаграмма: `flow.c4` → `flow_local_authz_bootstrap`

## Цель

Идемпотентно подготовить seller, двух bidders и минимальный Processes workload grant через owner gRPC-границу Authz, не выдавая seed-скрипту SpiceDB credential и не обходя workload policy.

## Сценарий

```mermaid
sequenceDiagram
    autonumber
    participant Job as gavel-authz-bootstrap
    participant Authz as Authz owner API
    participant Receipts as Authz receipts DB
    participant SpiceDB

    loop seller, bidder-one, bidder-two
        Job->>Authz: RegisterPrincipal(local issuer, subject, stable key)
        Authz->>Authz: Verify bootstrap workload + local command
        Authz->>Receipts: Claim canonical fingerprint
        alt Exact replay
            Receipts-->>Authz: Stored opaque principal + revision
        else First execution
            Authz->>SpiceDB: TOUCH platform:gavel#member@user:u1_…
            SpiceDB-->>Authz: Written-at revision
            Authz->>Receipts: Complete durable result
        end
        Authz-->>Job: Opaque principal id + revision
    end
    Job->>Authz: WriteRelationships(process_worker@processes, stable key)
    Authz->>Authz: Compile-time allowlist permits only fixed Processes grant
    Authz->>SpiceDB: TOUCH platform:gavel#process_worker@service_account:processes
    Job->>Authz: Check expected user/workload permissions at least as fresh
    Authz->>SpiceDB: Evaluate allow and negative administer check
    Authz-->>Job: Verified or fail closed
```

Повтор Job использует те же issuer/subjects/idempotency keys и получает сохранённые результаты без новых логических principals или relationship effects. Изменённый payload с прежним key конфликтует в receipt store.

Bootstrap не создаёт `lot`, `auction`, `payment_profile` или notification tuples: эти отношения принадлежат соответствующим owner services и создаются последующим domain seed через их API. Job не получает raw SpiceDB token, не пишет таблицы напрямую и не превращает Processes в administrator.

В local `kind` идентичность Job ограничена отдельным ServiceAccount, NetworkPolicy и development-only trusted pod CIDR. Production profile по умолчанию не рендерит Job и не включает эту identity.
