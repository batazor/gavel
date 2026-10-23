# @gavel/ui

UI-kit операционной админки и публичной панели торгов Gavel.

```bash
npm run storybook        # http://localhost:6006
npm run ui:check         # tsc
npm run storybook:build
```

## Направление

Стиль — **achromatic ledger** (выдан `design-taste-frontend`, seed 1728568553): чернила на тёплой каменной бумаге,
цвет только на данных. Токены в `src/styles.css` (`@theme`), заметки по направлению в `.taste-skill/notes.md` в корне репозитория.

- Шрифт Geist (Google Fonts), Geist Mono для ID. Табличные цифры везде.
- Медь — единственный оттенок, и только на главном элементе данных (цена, серия торгов, дедлайн).
- Одно основное действие на экран, почти чёрное. Границы 1px, радиус 8/6, теней в покое нет.

## Структура

- `src/components` — примитивы (Button, Panel + состояния, KPI, StatusBadge, Avatar), графики (LineChart, ClosingBoard),
  доменные блоки (LiveBidPanel, ClosingPolicyBuilder, SettlementRecoveryCard, AuctionFlow) и AdminShell.
- `src/pages` — пять экранов продукта на общих данных.
- `src/data` — демо-данные с инвариантами; заменяются проекцией BFF.

## Проверка

```bash
node ~/.claude/skills/design-taste-frontend/scripts/inspect.mjs --app "http://localhost:6006/iframe.html?id=pages-product-map--overview&viewMode=story"
```

Инспектор измеряет экран на 1440, 768 и 390 px и кладёт скриншоты в `.taste-skill/inspect/`.
