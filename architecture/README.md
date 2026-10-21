# Архитектура (LikeC4)

Архитектура описана как код в `*.c4` файлах. Это источник правды, картинки генерируются.

| Файл | Что внутри |
|---|---|
| `spec.c4` | Словарь: виды элементов, связей, тегов |
| `model.c4` | Логическая модель: люди, внешние системы, Gavel, bounded contexts |
| `deployment.c4` | Production: Kubernetes, namespace на bounded context, Istio, ArgoCD |
| `views.c4` | Диаграммы по уровням C4: C1 context, C2 containers, C3 components, deployment |
| `flows.c4` | Сценарии по шагам (dynamic views): ставка, выставление лота, переподключение SSE |

## Команды

```bash
npm run arch            # живой просмотр на http://localhost:5173
npm run arch:validate   # проверка синтаксиса и семантики
npm run arch:build      # статический сайт в site/
```

## Уровни C4

| Вид | Уровень | Что показывает |
|---|---|---|
| `index` | C1 | Gavel как чёрный ящик, пользователи, внешние системы |
| `containers` | C2 | Web UI, BFF, сервисы с БД по контекстам, брокер, ClickHouse |
| `bff`, `auction`, `catalog`, `analytics` | C3 | Компоненты внутри одного контейнера; `analytics` — поток в ClickHouse |
| `observability` | сквозное | Путь телеметрии |
| `prod`, `prod_mesh` | deployment | Kubernetes, Istio |
| `flow_*` | сценарии | Пошаговый граф или sequence-диаграмма, переключатель в UI. Кнопка Start проигрывает шаги |

C4-уровень «Code» не рисуем: его роль выполняют Rust-типы и Lean-спека.

Режим Graph в UI (force-раскладка точками) экспериментальный и из DSL не управляется, для чтения архитектуры используем Diagram и Sequence.

## Публикация

Workflow `.github/workflows/docs.yml` собирает сайт и выкладывает в GitHub Pages при push в `main`, если менялись `architecture/`, `docs/` или `VISION.md`. Адрес: `https://<owner>.github.io/gavel/`. ADR и use-cases доступны там же по `/docs/`.
Один раз включить в репозитории: Settings → Pages → Source: GitHub Actions.

## Соглашения

- Тег `#candidate` — решение не принято, элемент может исчезнуть. Список кандидатов в `VISION.md`.
- Тег `#domain` — чистая доменная логика, без инфраструктурных зависимостей.
- Тег `#mesh` — обеспечивается Istio, а не кодом приложения.
- Новый bounded context — новый `context` внутри `gavel` с `service` и `db` внутри, плюс view для него (ADR-0004).
- Виды читаются сверху вниз: клиент → BFF → адаптер → домен → репозиторий → БД. Стрелка = направление зависимости (кто кого вызывает или на кого подписан), поэтому подписчик → брокер. Циклов нет.
- Сквозные вещи (телеметрия, меш) в контекстные виды не включаются, у них свои виды: `observability`, `prod_mesh`.
- Deployment: namespace на bounded context, сервис и его БД рядом. Sidecar-ы envoy есть у каждого пода приложения, но в виде `prod` скрыты.
- Архитектурное решение сначала оформляется как ADR в `docs/adr/`, потом отражается в модели.
- Новый сценарий (SDD) — новый `dynamic view` в `flows.c4`. Заметки `notes` на шагах — место для инвариантов, которые потом уходят в Lean.
