# Архитектура (LikeC4)

Архитектура описана как код в `*.c4` файлах (`docs/architecture/`). Это источник правды, картинки генерируются.

| Файл | Что внутри |
|---|---|
| `spec.c4` | Словарь: виды элементов, связей, тегов |
| `model.c4` | Логическая модель: люди, внешние системы, Gavel, bounded contexts |
| `docs/architecture/deployment.c4` | Production: Kubernetes, namespace на bounded context, Istio, ArgoCD |
| `1-architecture/views.c4` | Диаграммы по уровням C4: C1 context, C2 containers, C3 components, телеметрия |
| `2-deployment/views.c4` | Deployment-виды: Kubernetes, Istio mesh |

Диаграммы юзкейсов (dynamic views) лежат рядом с их описанием: `contexts/<context>/use-cases/<case>/flow.c4`. Рабочая область LikeC4 — корень репозитория, поэтому сайдбар («By folders») повторяет структуру каталогов: `contexts` и `docs/architecture`. Внутри каталога виды идут по заголовку (natural sort).

## Команды

```bash
npm run arch            # живой просмотр на http://localhost:5173 (рабочая область — корень репозитория)
npm run arch:validate   # проверка синтаксиса и семантики
npm run arch:build      # статический сайт в site/
```

## Уровни C4

| Вид | Уровень | Что показывает |
|---|---|---|
| `index` | C1 | Gavel как чёрный ящик, пользователи, внешние системы |
| `containers` | C2 | Cloudflare edge, Web UI, Cosmo Router + Realtime subgraph, сервисы с БД по контекстам, Authz/SpiceDB, process worker + Temporal, Redpanda, ClickHouse |
| `bff`, `authz`, `auction`, `catalog`, `processes`, `payments`, `notify`, `analytics` | C3 | Компоненты проектов; `authz` — permission graph, `processes` — Temporal-слой, `analytics` — поток в ClickHouse |
| `observability` | сквозное | Путь телеметрии |
| `prod`, `prod_mesh` | deployment | Kubernetes, Istio |
| `flow_*` | юзкейсы | В папках юзкейсов. Пошаговый граф или sequence-диаграмма, переключатель в UI. Кнопка Start проигрывает шаги |

C4-уровень «Code» не рисуем: его роль выполняют Rust-типы и Lean-спека.

Режим Graph в UI (force-раскладка точками) экспериментальный и из DSL не управляется, для чтения архитектуры используем Diagram и Sequence.

## Публикация

Workflow `.github/workflows/docs.yml` собирает сайт и выкладывает в GitHub Pages при push в `main`, если менялись `docs/`, `contexts/` или `VISION.md`. Адрес: `https://<owner>.github.io/gavel/`. ADR и use-cases доступны там же по `/docs/`.

## Соглашения

- Тег `#candidate` — решение не принято, элемент может исчезнуть. Список кандидатов в `VISION.md`.
- Тег `#domain` — чистая доменная логика, без инфраструктурных зависимостей.
- Тег `#mesh` — обеспечивается Istio, а не кодом приложения.
- Новый bounded context — новый `context` внутри `gavel` с `service` и `db` внутри, плюс view для него (ADR-0004).
- Виды читаются сверху вниз: клиент → Cosmo Router → subgraph adapter → домен → репозиторий → БД. Стрелка = направление зависимости (кто кого вызывает или на кого подписан), поэтому подписчик → брокер. Циклов нет.
- Сквозные вещи (телеметрия, меш) в контекстные виды не включаются, у них свои виды: `observability`, `prod_mesh`.
- Deployment: namespace на bounded context, сервис и его БД рядом. Sidecar-ы envoy есть у каждого пода приложения, но в виде `prod` скрыты.
- Решение сначала оформляется рядом с владельцем: системное — в `docs/adr/`, локальное — в `contexts/<project>/adr/`; затем отражается в модели.
- Новый юзкейс — папка `use-cases/<case>/` с `README.md` и `flow.c4` (dynamic view). LikeC4 показывает место сценария в общей архитектуре и остаётся источником архитектурных views.
- Пошаговый основной сценарий внутри `README.md` оформляем встроенным `mermaid` `sequenceDiagram`, чтобы он рендерился непосредственно в GitHub и GitLab. Mermaid не заменяет `flow.c4`: README объясняет поведение, LikeC4 связывает его с общей моделью. Инварианты и исключения остаются текстом рядом с диаграммой.
