# @gavel/ui

UI-kit операционной админки и публичной панели торгов Gavel.

```bash
npm run storybook        # http://localhost:6006
npm run ui:check         # tsc
npm run storybook:build
```

## Направление

Композиция и правила экранов — из app-screens run `design-taste-frontend` (стиль achromatic ledger, seed 1728568553);
характер — свой: пергамент, тёмный сайдбар с медным молотком, медная кнопка. Токены в `src/styles.css` (`@theme`),
заметки по направлению в `.taste-skill/notes.md` в корне репозитория.

- Шрифт Avenir Next с Figtree (Google Fonts) как запасным, Geist Mono для ID. Табличные цифры везде.
- Медь — одно основное действие на экран и главный элемент данных (цена, серия торгов, дедлайн).
- Панели `.panel`: радиус 12, тёплая линия 1px, едва заметная тень. Контролы радиус 8.

## Структура

- `src/components` — примитивы (Button, Panel + состояния, KPI, StatusBadge, Avatar), графики (LineChart, ClosingBoard),
  доменные блоки (LiveBidPanel, ClosingPolicyBuilder, SettlementRecoveryCard, AuctionFlow) и AdminShell.
- `src/pages` — пять экранов продукта на общих данных.
- `src/data` — демо-данные с инвариантами; заменяются проекцией BFF.

## Проверка

```bash
node ~/.claude/skills/design-taste-frontend/scripts/inspect.mjs --app --allow font-not-dealt,micro-label,palette-cream,palette-claude "http://localhost:6006/iframe.html?id=pages-product-map--overview&viewMode=story"
```

Инспектор измеряет экран на 1440, 768 и 390 px и кладёт скриншоты в `.taste-skill/inspect/`. Четыре правила отключены
сознательно: шрифт бренда вместо выданного Geist, трекинг в словесном знаке GAVEL, пергаментный фон и медь на нём.
