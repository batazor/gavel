# Bounded contexts и юзкейсы

Структура: `contexts/<project>/README.md` — карта проекта; `contexts/<project>/adr/0001-init.md` — зачем создан проект, его требования и референсы; `contexts/<project>/use-cases/<use-case>/` — README с логикой и `flow.c4` с диаграммой. Код будет жить рядом с документацией проекта.

Статусы: **есть** — описан и в модели, **next** — следующая очередь, **later** — после MVP.

| Контекст | Юзкейс | Статус |
|---|---|---|
| [Catalog](catalog/README.md) | [Выставить лот и открыть торги](catalog/use-cases/list-lot/README.md) | есть |
| Catalog | [Загрузить изображение лота](catalog/use-cases/upload-lot-image/README.md) | есть |
| Catalog | [Снять лот](catalog/use-cases/withdraw-lot/README.md) | есть |
| Catalog | Редактировать лот до первой ставки | next |
| Catalog | Поиск и фильтрация лотов | later |
| Catalog | Модерация лота | later |
| [Auction](auction/README.md) | [Ставка](auction/use-cases/place-bid/README.md) | есть |
| Auction | [Получить текущую публичную цену](auction/use-cases/get-current-price/README.md) | есть |
| Auction | [Автоставка](auction/use-cases/proxy-bid/README.md) | есть |
| Auction | [Закрытие по дедлайну](auction/use-cases/close-by-deadline/README.md) | есть |
| Auction | [Продавец меняет время окончания](auction/use-cases/reschedule/README.md) | есть |
| Auction | Резервная цена | later |
| Auction | «Купить сейчас» | later |
| Auction | Отзыв ставки | не делаем |
| [Payments](payments/README.md) | [Подготовить платёжный инструмент до участия](payments/use-cases/register-payment-instrument/README.md) | есть |
| [Processes](processes/README.md) | [Оплата выигранного лота и переход к следующему кандидату](processes/use-cases/settle-auction/README.md) | есть |
| Processes | Следующий settlement candidate после неуспешной оплаты | есть, в `settle-auction` |
| [Notify](notify/README.md) | [Settlement-уведомления](notify/use-cases/settlement-notifications/README.md) | есть |
| Payments | Возврат, выплата продавцу | later |
| [Analytics](analytics/README.md) | [История цены и дашборды](analytics/use-cases/price-history/README.md) | есть |
| Analytics | [Полная публичная история ставок](analytics/use-cases/bid-history/README.md) | есть |
| [BFF](bff/README.md) | [Живое обновление цены и переподключение subscription](bff/use-cases/live-updates/README.md) | есть |
| BFF | Логин через OAuth | есть, authentication в меше |
| [Authz](authz/README.md) | [Авторизация пользовательской команды](authz/use-cases/authorize-command/README.md) | есть |
| Notify | [«Вас перебили»](notify/use-cases/outbid-notification/README.md) | есть |
| — | Watchlist | later |

Сквозные процессы (Temporal): [processes/README.md](processes/README.md). BFF и process worker — не bounded contexts в смысле DDD, но часть нашей границы, поэтому живут здесь же.
