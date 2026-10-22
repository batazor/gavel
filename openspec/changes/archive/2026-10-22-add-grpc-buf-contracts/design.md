# Design

## Context

См. `proposal.md` — Why. Сейчас Cosmo Router обращается к владельческим Federation subgraphs, Processes координирует Auction, Catalog, Payments и Notify, а Authz инкапсулирует SpiceDB. C4 показывает направления вызовов, но не фиксирует wire protocol; `.proto`, Buf configuration и compatibility checks отсутствуют.

## Goals / Non-Goals

**Goals:**

- дать каждому владельцу независимый versioned Protobuf module;
- стандартизировать внутренние unary RPC, ошибки, deadlines, retries и telemetry;
- обеспечить локальную и CI-проверку contracts до появления production traffic;
- оставить GraphQL, Temporal и Redpanda на их текущих архитектурных ролях.

**Non-Goals:**

- не заменять Federation subgraphs на gRPC и не добавлять custom business logic в Cosmo Router;
- не переводить public events в Protobuf в рамках этого change;
- не разворачивать BSR, gRPC gateway или новый ingress;
- не генерировать и не коммитить полноценную доменную реализацию RPC handlers.

## Decisions

### 1. Матрица транспортов

| Связь | Транспорт | Почему |
|---|---|---|
| Browser → Cosmo → owner subgraph | GraphQL/HTTP | Публичная Federation граница и query planning |
| Processes activities → Auction/Catalog/Payments/Notify | gRPC | Типизированные внутренние команды с deadlines и безопасными повторами |
| Owner services → Authz | gRPC | Внутренние Check/BulkCheck/relationship operations |
| Authz → SpiceDB | официальный gRPC API | Нативный transport зависимости |
| Processes worker → Temporal | Temporal SDK over gRPC | Нативный transport Temporal |
| Services → OTel Collector | OTLP/gRPC | Нативный telemetry transport |
| Owners → subscribers | Outbox → Redpanda | Асинхронные факты, replay и независимые consumers |
| Payment provider → Payments | HTTPS callback | Внешняя provider integration |

Альтернатива «gRPC между Cosmo и каждым доменом» отвергнута: Router ожидает Federation subgraph transport, а adapter создаст дополнительный hop без новой доменной границы. Альтернатива «всё через broker» отвергнута для команд, которым Temporal activity нужен authoritative result.

### 2. Один Buf v2 workspace, module на владельца

Корневой `buf.yaml` v2 объявляет modules:

- `contexts/auction/proto` → `gavel/auction/v1`;
- `contexts/catalog/proto` → `gavel/catalog/v1`;
- `contexts/payments/proto` → `gavel/payments/v1`;
- `contexts/notify/proto` → `gavel/notify/v1`;
- `contexts/authz/proto` → `gavel/authz/v1`.

В v2 отдельный устаревший `buf.work.yaml` не нужен. Общие `STANDARD` lint и `FILE` breaking rules задаются workspace configuration. `buf.gen.yaml` использует pinned code-generation plugins для Rust; generated output располагается в build directory и не редактируется вручную.

Отдельный `common.proto` не создаётся. Standard protobuf types используются для timestamp/duration; identifiers передаются как валидируемые opaque strings. Денежные значения определяются владельцем Payments/Auction как `minor_units + currency_code`; перевод между их контрактами выполняет Processes adapter, чтобы внутренние доменные типы не стали общей моделью.

Альтернатива одного общего module отвергнута: она размывает ownership и позволяет случайные импорты между контекстами. Отдельные repository-wide configs на каждый модуль отвергнуты как дублирование policy.

### 3. Начальные сервисные контракты

- Auction: `CloseAuction`, `CancelAuction`, `ForfeitSettlementCandidate`, `ConfirmWinner`.
- Catalog: `WithdrawLot`.
- Payments: `AttemptPayment`, `ExpirePayment`.
- Notify: `SendNotification`.
- Authz: `CheckPermission`, `BulkCheckPermissions`, `WriteRelationships`.

Messages содержат только устойчивые business identifiers и входы команды. Provider-specific payment objects, secret proxy maxima, storage keys, raw JWT и SpiceDB credentials запрещены. Processes импортирует modules владельцев и генерирует clients; его Temporal workflow types не публикуются как RPC contract.

### 4. Idempotency, consistency и recovery

Каждая mutation содержит `idempotency_key`; рекомендуемый ключ Processes — стабильная комбинация workflow id, run-independent logical operation и attempt/round, когда раунд является частью business identity. Владелец атомарно фиксирует domain change, idempotency result и outbox в своей Postgres transaction. Одинаковый ключ с другим request fingerprint возвращает typed conflict.

Deadline задаёт caller. Retry разрешён только для явно retryable codes (`UNAVAILABLE`, `RESOURCE_EXHAUSTED`, `DEADLINE_EXCEEDED`) и всегда использует тот же idempotency key. `INVALID_ARGUMENT`, `FAILED_PRECONDITION`, `NOT_FOUND`, `PERMISSION_DENIED` и typed domain details не повторяются автоматически. После ambiguous timeout caller повторяет запрос с тем же ключом вместо предположения о rollback.

### 5. Authorization и network boundary

Istio mTLS предоставляет verified workload identity. Owner adapter преобразует её в service account, вызывает Authz gRPC и только затем передаёт команду application layer. End-user identity, если нужна, приходит как подписанный token-exchange assertion либо повторно проверенный JWT согласно Authz ADR, но не как доверенный metadata header.

Authz остаётся единственным приложением с raw SpiceDB credentials. Его `WriteRelationships` ограничивает resource types по caller workload. Reflection доступна только внутри dev/test environment; production endpoint закрыт NetworkPolicy и mesh authorization policy.

### 6. Ошибки и валидация

Transport status отражает класс отказа, а стабильные domain details передаются typed protobuf details с machine-readable reason и безопасными metadata. Сообщения не содержат секреты. Proto validation rules проверяются на ingress adapter; domain layer повторно поддерживает инварианты независимо от transport.

### 7. Registry и lifecycle

Git является source of truth. CI сравнивает change с default branch через `buf breaking`, затем выполняет format/lint/build/generate verification. BSR — отдельный publication stage: hosted private или licensed on-prem deployment можно включить после появления нескольких независимых consumer release cycles.

BSR хранит source modules, labels, docs и generated SDK; Redpanda Schema Registry хранит runtime event schemas. RPC request/response types не регистрируются как Kafka event schemas. Это разделение будет отражено в глобальном ADR и `VISION.md` open decisions.

### 8. Observability

Server и client interceptors публикуют RED metrics с bounded labels `rpc_service`, `rpc_method`, `grpc_code`; business rejection учитывается отдельно от transport failure. OTel propagates trace context. Entity IDs, user IDs и idempotency keys запрещены в metric labels; допускаются в защищённых structured logs/traces согласно retention policy.

Каждый сервис добавляет RPC panels и alerts в собственные `contexts/<owner>/ops/Helm` dashboard/rules. Processes показывает client latency/retries рядом с Temporal activity metrics.

## Risks / Trade-offs

- [Дублирование похожих money/id types между modules] → сохранять ownership и писать явные adapter mappings; выделять shared module только после второго действительно общего стабильного контракта.
- [Retry storm усиливает отказ downstream] → обязательные deadlines, exponential backoff с jitter, ограничение attempts и mesh/client circuit breaking.
- [Кодогенерация расходится между разработчиками] → pinned plugins и CI check чистого generated result.
- [Buf on-prem добавит заметную эксплуатационную стоимость] → начать с CLI+Git, а BSR активировать отдельным ADR/deployment change.
- [gRPC становится скрытым обходом событий] → C4/ADR review требует для каждого вызова классификацию command/query или event и явного владельца.

## Migration Plan

1. Добавить ADR, Buf workspace/configuration и пустой baseline для breaking comparison.
2. Добавить modules владельцев и сгенерировать компилируемые Rust stubs.
3. Подключить CI checks без runtime routing changes.
4. Обновить init ADR, README и C4 labels; синхронизировать Mermaid use cases.
5. В последующих changes реализовывать handlers и переводить Processes activities по одной операции, сохраняя старый путь до проверки parity.
6. Rollback runtime отсутствует: этот change добавляет contracts и проверки, не переключая production traffic. Неудачный contract до первого consumer можно удалить; после публикации `v1` используется только additive evolution либо новый package.
