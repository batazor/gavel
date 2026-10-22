# @gavel/ui

Прототип UI-kit для Web UI и операционной админки Gavel.

```bash
npm run storybook
npm run ui:check
npm run storybook:build
```

Storybook открывается на `http://localhost:6006`. В `Prototype/Admin directions` доступны три компоновки; стрелки на экране и клавиши `←` / `→` меняют `?variant=A|B|C`.

В `Prototype/Complex components` тем же способом переключаются три доменных контекста: live bidding, closing policy и settlement recovery. Изолированные состояния компонентов находятся в разделе `Domain`.

`Prototype/Product pages` объединяет пять экранов: создание лота, управление лотами, управление клиентами, профиль клиента и overview админки. Используйте `?variant=A|B|C|D|E` или экранные стрелки.

Для responsive-проверки в Storybook добавлены viewport-пресеты `Tablet compact` (768 × 1024), `Tablet comfortable` (834 × 1194) и `Tablet landscape` (1024 × 768). На планшетах `AdminShell` переходит в компактную rail-навигацию, а составные доменные компоненты используют container queries.

Пакет намеренно остаётся прототипом: после выбора направления компоненты-победители нужно адаптировать к будущему Astro Web UI, а лабораторию вариантов вынести из основной ветки.
