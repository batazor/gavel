# 18. Internal RPC with gRPC and Buf

Date: 2026-10-22

## Status

Accepted

## Контекст

Processes синхронно отправляет команды владельцам Auction, Catalog, Payments и Notify, а доменные проекты синхронно проверяют permissions через Authz. Без общего transport contract эти вызовы могут смешаться с публичным GraphQL API или асинхронными событиями и потерять правила совместимости, повторов и владения.

## Решение

Внутренние синхронные command/query interfaces используют unary gRPC с versioned Protobuf contracts. Каждый contract принадлежит проекту, который принимает окончательное доменное решение:

- Auction владеет командами закрытия, отмены, forfeiture и подтверждения победителя;
- Catalog владеет снятием лота;
- Payments владеет попыткой и истечением оплаты;
- Notify владеет доставкой уведомления;
- Authz владеет permission checks и записью relationships.

Processes генерирует clients из modules владельцев и не определяет копии их messages. Общего domain/common module нет: устойчивые opaque ids и денежные значения переводятся adapter-ами каждого контекста.

### Матрица транспортов

| Связь | Транспорт |
|---|---|
| Browser → Cosmo Router → owner subgraph | GraphQL/HTTP |
| Processes activities → Auction/Catalog/Payments/Notify | gRPC |
| Owner contexts → Authz | gRPC |
| Authz → SpiceDB | официальный SpiceDB gRPC API |
| Processes worker → Temporal | Temporal SDK over gRPC |
| Services → OTel Collector | OTLP/gRPC |
| Owner contexts → subscribers | transactional outbox → Redpanda |
| Payment provider → Payments | HTTPS callback |

Cosmo Router не вызывает доменные gRPC endpoints и не содержит business rules. Браузеру внутренние RPC endpoints не публикуются. Факт, возникший после RPC-команды, доставляется отдельным versioned event contract через outbox и Redpanda; RPC response не переиспользуется как событие.

### Buf и совместимость

Корневой Buf v2 workspace содержит отдельный module для каждого владельца. Git является source of truth. CI выполняет `buf format`, `buf lint`, `buf build` и `buf breaking` относительно default label в BSR. Проверенный module commit сначала публикуется под preview label изменения, а после consumer/package gates продвигается на release/default label.

После появления consumer у пакета `v1` разрешены только additive changes. Удаление поля, изменение wire type или несовместимое изменение service требует нового versioned package. Сгенерированный Rust code не коммитится. Owner и consumers используют BSR Generated SDK через sparse Cargo registry `https://buf.build/gen/cargo/public/`; package name разрешается командой `buf registry sdk`, а dependency фиксируется exact version, содержащей module commit. `Cargo.lock` и context-owned `bsr-sdk.lock.json` сохраняют provenance и rollback version. Floating SDK versions и локальная компиляция owner proto в consumer запрещены.

Buf Schema Registry хранит source RPC modules, documentation и generated SDK. Redpanda Schema Registry хранит runtime event schemas. Эти registry не заменяют друг друга; публикация в BSR и выбор формата event schemas принимаются отдельными решениями.

### Идемпотентность и recovery

Каждая изменяющая команда принимает стабильный `idempotency_key`. Владелец атомарно сохраняет domain change, результат идемпотентности и outbox. Повтор с тем же key и тем же request fingerprint возвращает ранее сохранённый результат; тот же key с другим payload возвращает typed conflict без изменения состояния.

Caller всегда задаёт deadline. Автоматический retry разрешён для `UNAVAILABLE`, `RESOURCE_EXHAUSTED` и `DEADLINE_EXCEEDED`, использует тот же idempotency key, exponential backoff с jitter и ограниченное число попыток. После ambiguous timeout caller повторяет запрос, а не предполагает rollback. Domain rejection через `INVALID_ARGUMENT`, `FAILED_PRECONDITION`, `NOT_FOUND`, `PERMISSION_DENIED` или typed details автоматически не повторяется.

### Authorization

Owner adapter получает workload identity из Istio mTLS и преобразует её в доверенный service principal. Произвольный principal из payload или metadata не считается доверенным. Перед mutation владелец выполняет authoritative check через Authz; при недоступности Authz mutation fail closed до доменного изменения.

Только Authz имеет raw SpiceDB credentials. `WriteRelationships` ограничивает допустимые resource types по workload вызывающего проекта. Reflection допускается только в dev/test; production endpoint закрыт mesh authorization policy и NetworkPolicy.

Catalog `WithdrawLot` принимает caller только из server extension, установленного trusted-proxy SPIFFE interceptor. Разрешён единственный workload `spiffe://…/gavel-processes`; raw `x-workload`/principal metadata игнорируется. Запрос обязан содержать положительный `grpc-timeout` не больше service limit, idempotency key и correlation id. Domain rejection возвращается typed `oneof`, а timeout/unavailable остаётся transport status без backend details.

### Ошибки и наблюдаемость

gRPC status отражает transport/application class, а стабильный domain reason передаётся typed details без секретов. Валидация transport DTO не заменяет доменные инварианты.

Client и server interceptors передают OTel trace context и публикуют RED metrics с bounded labels `rpc_service`, `rpc_method`, `grpc_code`. Entity ids, user ids и idempotency keys запрещены в metric labels. Domain rejections учитываются отдельно от transport failures.

## Последствия

- Внутренние команды типизированы и имеют одного владельца.
- Temporal retries безопасны только при корректной owner-side idempotency.
- Похожие money/id messages могут дублироваться между modules, сохраняя независимость bounded contexts.
- gRPC не заменяет GraphQL, Temporal или Redpanda и добавляет ещё один contract lifecycle в CI.
- Доступ к private BSR и публикация получают `BUF_TOKEN` только из внешнего CI/developer credential provider; credentials не хранятся в repository.

## Rollout

1. Добавить Buf workspace и owner modules.
2. Включить format/lint/build/breaking checks.
3. Опубликовать preview module commit, разрешить Generated SDK package names и зафиксировать exact versions.
4. Реализовывать owner handlers и переключать Temporal activities по одной операции.
5. До проверки parity сохранять существующий путь вызова; после публикации `v1` изменять contracts только совместимо.
6. При rollback вернуть обе exact SDK versions из context lock-файла; module commit в BSR остаётся immutable.

## References

- [ADR-0004 — Service per bounded context](0004-service-per-bounded-context.md)
- [ADR-0005 — Transactional outbox](0005-transactional-outbox-for-integration.md)
- [ADR-0012 — Redpanda](0012-redpanda-as-message-broker.md)
- [ADR-0015 — Fine-grained authorization](0015-fine-grained-authorization-with-spicedb.md)
- [BFF](../../contexts/bff/README.md)
- [Processes](../../contexts/processes/README.md)
