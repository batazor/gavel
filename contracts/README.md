# Cross-context contracts

Этот каталог принадлежит общей интеграционной границе Gavel. Доменные protobuf,
Federation SDL и SpiceDB schema по-прежнему принадлежат соответствующим bounded
contexts; здесь хранится только проверяемый registry маршрутов и immutable pins,
которые связывают владельцев в локальном MVP.

## Event topics

[`event-topics.json`](event-topics.json) фиксирует шесть маршрутов MVP:

- lifecycle events Catalog для Auction;
- payment-instrument readiness events Payments для Auction;
- публичные lifecycle events Auction;
- restricted settlement events Auction только для Processes;
- events payment attempts Payments только для Processes;
- sanitized quarantine/DLQ records, которыми владеет Processes.

Каждая запись задаёт владельца, schema/message, schema version, partition key,
ACL и compatibility policy. Broker bootstrap обязан материализовать ACL из этого
registry с deny-by-default; registry не является способом раздать payload другим
consumer-ам.

Fixtures в [`fixtures/`](fixtures/) показывают wire JSON mapping protobuf
envelopes. Quarantine fixture намеренно содержит только ссылочную диагностику и
никогда не копирует исходный payload; его строгий JSON Schema находится в
[`schemas/`](schemas/).

## Buf/BSR pins

[`bsr-dependencies.json`](bsr-dependencies.json) перечисляет exact Generated SDK
versions и pinned remote plugins. Version с префиксом `=` в Cargo manifest и
module commit, встроенный в Generated SDK version, являются воспроизводимым pin;
floating labels и version ranges запрещены. Полный Payments provenance остаётся
в `contexts/payments/bsr-sdk.lock.json`.

Federation и SpiceDB для MVP не требуют новых прав или публичных полей:
существующие `Lot.auction`, `placeBid`, viewer-safe Auction fields и
`platform#process_worker` permissions достаточны. Это решение проверяется
`contract-check`, чтобы интеграционная работа не расширяла contracts без
реального consumer requirement.

Запуск всех лёгких проверок этого каталога отложен до финального verification:

```bash
make contract-check
```
