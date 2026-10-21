# 1. Record architecture decisions

Date: 2026-10-21

## Status

Accepted

## Context

Проект обкатывает много практик сразу (SDD, DDD, Lean, Istio, OTLP). Решения и их мотивация должны быть зафиксированы, иначе через месяц непонятно, почему сделано так.

## Decision

Используем Architecture Decision Records в формате Michael Nygard. Файлы в `docs/adr/`, имя `NNNN-short-title.md`, статусы: Proposed, Accepted, Deprecated, Superseded.

## Consequences

Каждое решение из «Открытых вопросов» в VISION.md оформляется отдельным ADR. Изменение принятого решения — новый ADR со ссылкой на старый.
