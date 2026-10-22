# Proposal

## Why

В архитектуре уже есть синхронные внутренние вызовы Processes и Authz, но их транспорт, владение контрактами и правила совместимости не определены. Без общего решения команды быстро смешают GraphQL, HTTP и события либо создадут неуправляемые `.proto`, поэтому до реализации интерфейсов нужно закрепить gRPC и Buf как единый внутренний RPC-подход.

## What Changes

- Ввести gRPC только для синхронных внутренних command/query interfaces; публичный Browser API остаётся GraphQL Federation, факты остаются событиями Redpanda, provider callbacks — HTTP.
- Добавить глобальный ADR о границах gRPC, Buf governance, deadlines, идемпотентности, ошибках, авторизации, совместимости и rollout.
- Добавить Buf v2 workspace и отдельные владельческие modules для Auction, Catalog, Payments, Notify и Authz.
- Определить начальные versioned Protobuf contracts для process activities и authorization API; Processes генерирует клиентов, но не владеет сообщениями сервисов-получателей.
- Проверять `buf format`, `buf lint`, `buf build` и backward compatibility в CI; Git остаётся исходным source of truth.
- Разделить Buf Schema Registry для RPC source contracts и Redpanda Schema Registry для runtime event contracts. BSR допускается как следующий этап публикации; on-prem BSR сейчас не разворачивается.
- Обновить README, init ADR и LikeC4 связи затронутых проектов так, чтобы транспорт и владелец каждого вызова были явными.

## Capabilities

### New Capabilities

- `internal-rpc-contracts`: Правила и наблюдаемое поведение типизированных синхронных межсервисных команд Gavel, включая совместимость, повторы, deadlines и авторизацию.

### Modified Capabilities

Нет существующих capability specs.

## Impact

Основной владелец изменения — shared architecture в `docs/adr/` и корневой Buf workspace. Владельцы RPC contracts: `contexts/auction`, `contexts/catalog`, `contexts/payments`, `contexts/notify`, `contexts/authz`. `contexts/processes` потребляет доменные RPC и продолжает владеть Temporal orchestration; `contexts/bff` сохраняет GraphQL/Federation transport и документирует отсутствие gRPC на публичной границе. Analytics, публичная GraphQL schema, event payloads Redpanda, payment-provider callbacks и развёртывание BSR не входят в изменение.

Начальное добавление RPC совместимо, так как существующих wire contracts нет. После публикации `v1` изменения должны быть additive; breaking change требует нового versioned package. Сгенерированный код является build artifact и не становится владельцем контракта.
