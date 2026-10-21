# 2. Rate limiting via Istio service mesh

Date: 2026-10-21

## Status

Accepted

## Context

Ставки нужно ограничивать по частоте (защита от злоупотреблений и перегрузки). Реализация в коде приложения смешивает инфраструктурную заботу с доменной логикой и дублируется на каждый сервис.

## Decision

Rate limiting выносится в service mesh — Istio (Envoy local/global rate limit). Вместе с ним на меш переходят mTLS, retries, timeouts и распространение trace-контекста. Приложение не содержит логики лимитов.

## Consequences

- Домен остаётся чистым, лимиты настраиваются декларативно через `EnvoyFilter` / `RateLimit` CRD.
- Локальная разработка требует Istio в kind или честного эмулирования (лимиты не проверяются в unit-тестах).
- Лимит по `user_id` из JWT требует, чтобы Istio валидировал токен (`RequestAuthentication`) и вытаскивал claim в дескриптор.
