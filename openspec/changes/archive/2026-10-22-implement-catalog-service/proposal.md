# Proposal

## Why

Catalog пока остаётся hello-world binary при уже опубликованных GraphQL, gRPC и `LotListed` contracts. Без исполняемого владельца лота невозможно пройти MVP-путь загрузки изображений, выставления карточки и безопасного снятия через saga, поэтому следующий vertical slice должен превратить принятые DDD-решения в работающий bounded context.

## What Changes

- Реализовать независимый domain/application core Catalog для `Lot` и `LotImage`: value objects, состояния, инварианты, команды и стабильную taxonomy ошибок без зависимостей от transport/storage.
- Реализовать request/complete lifecycle прямой загрузки изображений через узкий S3-compatible port, ownership/metadata/checksum validation, TTL и reconciliation для orphan objects.
- Реализовать идемпотентные `ListLot`, `UpdateLot` и внутренний `WithdrawLot`: PostgreSQL unit of work атомарно сохраняет aggregate, command receipt, image attachment и outbox.
- До создания лота выполнить authoritative Authz check и owner-scoped provisioning `lot:platform`/`lot:seller`; сохранить returned revision для causal checks и безопасно восстанавливать orphan tuple через reconciliation.
- Реализовать Federation subgraph queries/mutations с verified principal context, cursor pagination и privacy-safe typed errors; Cosmo Router остаётся без бизнес-правил.
- Сохранить additive `gavel.catalog.v1` contract для Processes, реализовать trusted workload gRPC boundary, deadline/idempotency/error mapping и E2E contract с generated client.
- Публиковать versioned `LotListed`, `LotUpdated` и `LotWithdrawn` через transactional outbox; `LotListed v1` должен оставаться совместимым с существующим Auction consumer fixture.
- Заменить bootstrap production runtime, migrations, image/Helm, probes, external secrets, telemetry/dashboard/runbook и добавить disposable PostgreSQL/S3-compatible integration suites.
- Синхронизировать Catalog README/ADR/Mermaid/LikeC4 с фактическими транзакционными и cross-context границами.

## Capabilities

### New Capabilities

- `catalog-media-management`: безопасный lifecycle PENDING → READY → ATTACHED для принадлежащих продавцу изображений, прямой upload и очистка незавершённых объектов.
- `catalog-lot-lifecycle`: создание, редактирование и снятие лота с authorization, идемпотентностью, атомарным outbox и стабильными доменными отказами.
- `catalog-lot-discovery`: публичное чтение карточек и cursor-пагинация опубликованных лотов без раскрытия внутренних storage/authz данных.

### Modified Capabilities

- `internal-rpc-contracts`: конкретизировать совместимое поведение Catalog `WithdrawLot` для Processes, trusted workload identity, replay/conflict и authoritative typed outcomes.

## Impact

- **Primary owner — Catalog:** `contexts/catalog/` domain/application/adapters/runtime, GraphQL SDL, gRPC, migrations, object-storage boundary, outbox, tests, Helm и owned docs.
- **Authz:** используется существующий `gavel.authz.v1` client contract для checks и scoped relationship writes; schema и Authz ownership не меняются.
- **Auction:** существующий asynchronous `LotListed v1` consumer остаётся authoritative владельцем открытия торгов; fixture/contract tests подтверждают совместимость, синхронного Catalog → Auction вызова не добавляется.
- **Processes:** продолжает вызывать additive Catalog-owned `gavel.catalog.v1.WithdrawLot` после `CancelAuction`; orchestration/compensation реализуется отдельным Processes change.
- **Shared contracts/docs:** root Buf workspace и `docs/architecture/` обновляются только для реализованных Catalog boundaries и flows.
- **Dependencies/systems:** Catalog-owned PostgreSQL, S3-compatible object storage, Authz gRPC, Debezium/Redpanda outbox contract и external secret/config references.
- **Migration/compatibility:** forward-only SQL migrations; GraphQL/proto/event changes только additive. Существующие `LotListed v1` field names и semantics сохраняются.
- **Out of scope:** торговые инварианты и факт наличия ставок, Temporal workflow implementation, browser OAuth flow, image transformation/CDN, production credentials и фактическое production deployment.
