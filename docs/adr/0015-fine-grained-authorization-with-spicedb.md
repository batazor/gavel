# 15. Fine-grained authorization with SpiceDB

Date: 2026-10-22

## Status

Accepted

## Context

OAuth/OIDC и JWT подтверждают личность, но не отвечают на resource-level вопросы: кто может изменить или снять конкретный лот, увидеть свою платёжную попытку, продолжить recovery или выполнить системную команду Auction. Локальные RBAC-проверки дублировали бы модель доступа в BFF и каждом владельце ресурса; JWT roles устаревают до истечения токена и плохо выражают связи между ресурсами.

## Decision

- Использовать Authz service поверх SpiceDB как единый policy decision point для fine-grained authorization и scoped relationship writes.
- Оставить OAuth/OIDC источником authentication. Канонический principal строится только из проверенных `iss` + `sub`; JWT role claims не являются источником permissions.
- Хранить relations и вычислять permissions по схеме [`contexts/authz/schema/gavel.zed`](../../contexts/authz/schema/gavel.zed).
- Не выдавать raw SpiceDB write credentials доменным сервисам: Authz writer проверяет mTLS workload identity и допустимый resource type владельца.
- Размещать authoritative policy enforcement point в сервисе-владельце команды. Edge или owner subgraph может выполнить pre-check/BulkCheck для UX и viewer capabilities, но его allow не заменяет сервисную проверку.
- Передавать user identity до owner service исходным JWT или подписанным token-exchange assertion и проверять её на принимающей mesh-boundary; raw identity headers не являются trust boundary.
- Не моделировать через SpiceDB domain eligibility и mutable state machine. Allow означает «этому principal разрешено вызвать capability», после чего домен может вернуть `409`/`422`.
- Использовать ZedTokens и `at_least_as_fresh` для causal read-after-write; изменения доступа, где stale allow опасен, проверять `fully_consistent` до завершения mutation.
- Изменять schema через backward-compatible expand/migrate/contract; валидировать `.zed` и positive/negative assertions в CI.

## Consequences

- Authz становится общей synchronous dependency для private reads и mutations; они fail closed, публичные reads остаются доступными.
- Владельцы ресурсов отвечают за idempotent relationship provisioning и reconciliation, а Authz project — за schema lifecycle и SpiceDB availability.
- Схема permissions является межпроектным контрактом, но доменные данные не переносятся в authorization graph целиком.

## References

- [Authz init](../../contexts/authz/adr/0001-init.md)
- [SpiceDB schema language](https://authzed.com/docs/spicedb/concepts/schema)
- [SpiceDB consistency](https://authzed.com/docs/spicedb/concepts/consistency)
- [Developing and validating a schema](https://authzed.com/docs/spicedb/modeling/developing-a-schema)
