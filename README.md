# Gavel

Учебно-исследовательский аукцион. Английские торги с автоставками, event sourcing, формальные доказательства инвариантов в Lean, Cloudflare edge, Rust-микросервисы в Kubernetes с Istio, Temporal для процессов, Redpanda, ClickHouse и сменные платёжные провайдеры.

Цели и принятые решения — в [VISION.md](VISION.md).

## Карта репозитория

| Путь | Что там |
|---|---|
| [VISION.md](VISION.md) | Что и как строим, список решений, кандидаты, открытые вопросы |
| [docs/architecture/](docs/architecture/README.md) | Модель LikeC4: C1–C3, deployment. Источник правды по архитектуре |
| [contexts/](contexts/README.md) | Bounded contexts по DDD. У каждого README и папки юзкейсов с описанием и диаграммой |
| [openspec/](openspec/) | Единый repo-local OpenSpec root монорепозитория: changes, capability specs и SDD-конфигурация всех контекстов |
| [contexts/bff/](contexts/bff/README.md) | Self-hosted WunderGraph Cosmo Router, Federation composition и Realtime subgraph |
| [contexts/processes/](contexts/processes/README.md) | Слой процессов на Temporal: жизненный цикл аукциона, саги |
| [deploy/charts/observability/](deploy/charts/observability/README.md) | Prometheus Operator, alerts и Grafana dashboards для Kubernetes |
| [deploy/compose/observability/](deploy/compose/observability/README.md) | Локальный импорт тех же rules/dashboard в Prometheus, Alertmanager и Grafana |
| [contexts/authz/](contexts/authz/README.md) | Fine-grained authorization на SpiceDB и проверяемая permission schema |
| [GLOSSARY-MAP.md](GLOSSARY-MAP.md) | Карта ubiquitous language проектов |
| [docs/adr/](docs/adr/README.md) | Системные ADR: решения между проектами, общая архитектура, delivery и репозиторий |
| `contexts/<project>/ops/` | Multi-stage Dockerfile и project-owned Helm, Prometheus rules, Grafana dashboard |
| `.github/workflows/` | Экономный CI и публикация документации в GitHub Pages |

## Документация

```bash
make help
```

## Web UI

Операционная админка собрана из `@gavel/ui` в отдельном React-приложении:

```bash
npm install
npm run web
```

Приложение открывается на `http://127.0.0.1:4173/admin`. Основные маршруты: `/admin/lots`, `/admin/lots/new`, `/admin/customers`, `/admin/settlements` и `/admin/workflows`.

Автономный logic-прототип операторских переходов находится в `packages/web/prototypes/operations-flow.prototype.html`; он открывается напрямую в браузере и не требует dev-сервера.

```bash
npm run web:check
npm run web:build
```

Опубликованная версия: `https://batazor.github.io/gavel/`.

## Как добавить

- **Системное решение** — новый ADR в `docs/adr/`; решение одного проекта — в `contexts/<project>/adr/`. После этого изменение отражается в `docs/architecture/`.
- **Изменение поведения или интерфейса** — новый OpenSpec change в `openspec/changes/`; после реализации и проверки change архивируется, а capability specs синхронизируются в `openspec/specs/`.
- **Юзкейс** — папка `contexts/<context>/use-cases/<case>/` с `README.md` и `flow.c4`, строка в `contexts/README.md`.
- **Проект** — `contexts/<project>/README.md`, локальный `Makefile` и `adr/0001-init.md` по образцу `auction`; для bounded context также элемент `context` в `docs/architecture/model.c4` и namespace в `docs/architecture/deployment.c4`.

## Статус

Этап проектирования. У каждого проекта уже есть собираемый Rust hello-world runtime и эксплуатационный scaffold; доменные реализации и Lean-спека Auction остаются следующими шагами.
