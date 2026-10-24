# Tasks

## 1. Auction: authoritative outbid fact и producer contract

- [x] 1.1 Добавить `BidderOutbid` в private Auction domain event/codec и покрыть replay/no-op apply тестами, подтвердив, что event не хранит maximum или identity нового лидера через `make -C contexts/auction test`.
- [x] 1.2 Изменить `placeBid` decision: создать один outbid fact для прежнего лидера при смене лидерства либо для нового bidder при immediate proxy outbid, но не для повышения maximum текущим лидером; покрыть обе положительные ветки, отсутствие события и idempotent command replay unit/application тестами.
- [x] 1.3 Расширить `auction_process_events.proto`, restricted outbox encoder и `contracts/event-topics.json` additive `BidderOutbid(recipient_id,bid_id)` contract; проверить round-trip/privacy, `make buf-check`, `make buf-breaking` и `make contract-check`.
- [x] 1.4 Обновить `contexts/auction/use-cases/place-bid/README.md` и `flow.c4` restricted outbid-сценарием, сохранив public GraphQL SDL/events без identity; проверить документацию через `make arch-validate` и Auction check.

## 2. Notify: безопасный provider-neutral template

- [x] 2.1 Добавить protobuf/domain mapping `BidderOutbidV1` с version 1, сохранив совместимость `SendNotification`; покрыть accepted/unknown enum и canonical fingerprint тестами и прогнать `make buf-check` и `make -C contexts/notify test`.
- [x] 2.2 Реализовать `bidder_outbid.v1` renderer с обязательным `auction_id` и per-template allowlist data keys; тестами доказать рендеринг и fail-closed отказ для maximum, leader identity и произвольного extra field до вызова channel adapter.
- [x] 2.3 Добавить Notify service/adapter tests для exact replay, payload conflict, terminal operator read и transient/permanent delivery outcomes нового template; проверить `make -C contexts/notify check`.
- [x] 2.4 Создать `contexts/notify/use-cases/outbid-notification/README.md` и `flow.c4`, добавить юзкейс в `contexts/notify/README.md`; проверить Mermaid/LikeC4 согласованность через `make arch-validate`.

## 3. Processes: durable outbid orchestration

- [x] 3.1 Декодировать restricted `BidderOutbid` в отдельный owner event с валидацией recipient/bid/version и тестами valid, malformed, unsupported и wrong-source/quarantine paths; прогнать `make -C contexts/processes test`.
- [x] 3.2 Добавить `NotifyOutbid` route/workflow/task registration с workflow id `notify-outbid/{auction}:{version}` и stable request id; тестами подтвердить durable acceptance до broker commit, duplicate delivery и отсутствие сигналов в lifecycle/settlement workflows.
- [x] 3.3 Добавить outbid Notify activity с notification id `outbid/{auction}:{version}`, безопасным `auction_id`, существующей workload authorization и bounded retry; покрыть lost-response replay, idempotency conflict и permanent failure без Auction owner-команд.
- [x] 3.4 Добавить bounded telemetry для outbid event/workflow/template outcomes без recipient, auction/bid/notification ids и body в labels/logs; обновить Processes observability assertions и проверить `make -C contexts/processes observability-check` и `make -C contexts/processes check`.

## 4. Shared architecture и локальная интеграция

- [x] 4.1 Обновить `docs/architecture/model.c4`, новый Notify flow и `contexts/README.md` (`«вас перебили»` → `есть`), явно подтвердив отсутствие изменений public GraphQL SDL, SpiceDB permissions и public Auction events; проверить `make arch-validate`, `make graphql-check` и `make authz-validate`.
- [x] 4.2 Расширить local MVP smoke/fixtures сценарием: прежний лидер получает одно письмо после смены лидера, новый bidder получает одно письмо при immediate outbid, duplicate owner delivery не дублирует email, а повышение maximum лидером не отправляет письмо; проверить `make mvp-smoke` и safe operator read.
- [x] 4.3 Выполнить `make services-check`, `make contract-check`, `make openspec-validate` и `make mvp-ready`; недоступные environment-dependent проверки и причины зафиксировать в implementation handoff до завершения change.

## Workflow follow-up

- После review синхронизировать delta specs и архивировать change только после успешных contract/service checks и подтверждённого local delivery сценария.
