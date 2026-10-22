# 2. Layered rate limiting via Cloudflare, Cosmo and Istio

Date: 2026-10-21

## Status

Accepted

## Context

Ставки нужно ограничивать по частоте, а GraphQL защищать от дешёвых частых и редких дорогих запросов. Один слой не видит всех нужных признаков: Cloudflare знает network/client reputation, Cosmo знает GraphQL operation, Istio и owner service знают verified principal и состояние origin.

## Decision

Использовать layered controls:

- Cloudflare отсекает volumetric, bot и anonymous/IP abuse до origin;
- Cosmo ограничивает persisted operations, query cost/depth и concurrent work;
- Istio/Envoy применяет per-principal и service-level limits после JWT verification;
- owner service выполняет только domain eligibility и overload rejection, но не реализует infrastructure counters.

mTLS, retries, timeouts и trace context остаются в service mesh. Подробности публичного edge — в [ADR-0016](0016-cloudflare-as-public-edge.md).

## Consequences

- Домен остаётся чистым; edge/mesh лимиты настраиваются декларативно.
- Локальная разработка требует Istio в kind или честного эмулирования (лимиты не проверяются в unit-тестах).
- Лимит по `user_id` из JWT требует, чтобы Istio валидировал токен (`RequestAuthentication`) и вытаскивал claim в дескриптор.
- Cloudflare limit не заменяет per-principal limit: IP может быть общим, а attacker — распределённым.
- GraphQL query-cost и subscription connection limits проверяются отдельными k6 профилями.
