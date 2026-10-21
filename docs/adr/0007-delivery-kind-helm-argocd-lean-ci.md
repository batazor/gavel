# 7. Delivery: kind locally, Helm + ArgoCD in cluster, economical CI

Date: 2026-10-21

## Status

Accepted

## Context

Проект open source, лимиты бесплатных CI-минут ограничены. При этом стек тяжёлый: Rust-сборка, Lean-проверка, Istio в кластере. Нужно обозначить полный pipeline, но не платить за него на каждый push.

## Decision

- **Локальная разработка — kind.** Один скрипт поднимает кластер с Istio, OTel Collector, Postgres и брокером. Это же окружение используется для интеграционных тестов и записи Keploy.
- **Деплой — Helm + ArgoCD.** По Helm-чарту на сервис, umbrella-чарт на систему. ArgoCD синхронизирует кластер с веткой `main` (GitOps), руками в кластер не ходим.
- **CI — минимум на каждый PR, остальное заглушками.** На PR реально выполняется только дешёвое: `cargo fmt --check`, `cargo clippy`, `likec4 validate`. Тесты, Lean, сборка образов и helm lint обозначены в workflow как шаги с `if: false` или `workflow_dispatch`, чтобы pipeline читался целиком, но не тратил минуты. Включаем по мере надобности.

- **Документация — GitHub Pages.** Отдельный workflow `docs.yml` собирает LikeC4-сайт и кладёт рядом ADR. Запускается только при push в `main` с изменениями в `architecture/`, `docs/` или `VISION.md`, около минуты.

## Consequences

- Полный прогон (тесты, Lean, образы) делается локально перед PR. Это дисциплина, а не автоматика.
- Структура workflow описывает целевой pipeline, и когда появятся минуты или self-hosted runner, достаточно убрать `if: false`.
- ArgoCD добавляет namespace в кластер и Application-манифесты в репозиторий.
