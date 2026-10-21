# 6. Realtime updates via Server-Sent Events

Date: 2026-10-21

## Status

Accepted. Уточнён ADR-0008: публичный SSE-endpoint перенесён в BFF, auction-service отдаёт события в брокер и по запросу с версии.

## Context

Участник должен видеть новую цену лота без перезагрузки. Поток данных почти целиком односторонний: сервер → клиент. Ставка от клиента — редкое действие, которому и так нужны аутентификация, идемпотентность и ответ с результатом, то есть обычный HTTP POST.

Рассматривали WebSocket и SSE. WebSocket — отдельный протокол после upgrade, двунаправленный, нужен для частого обмена в обе стороны (игры, трейдинг-терминалы). SSE — обычный HTTP-ответ `text/event-stream`, односторонний, с встроенным переподключением и `Last-Event-ID` в браузерном `EventSource`. Поверх HTTP/2 ограничение в 6 соединений на домен не действует.

## Decision

- **SSE** для обновлений цены и статуса лота. Endpoint `GET /auctions/{id}/events` в auction-service, источник — зафиксированные события из event store (ADR-0005). `id` события = версия потока, поэтому `Last-Event-ID` при переподключении даёт клиенту пропущенные ставки без отдельного API.
- **Ставки — `POST`** с idempotency key. Ответ синхронный: принята или отклонена с причиной.
- **Istio:** для SSE-маршрута `timeout: 0s` в VirtualService и heartbeat-комментарий раз в 15–30 секунд, иначе Envoy закрывает тихий поток по `stream_idle_timeout` примерно через 5 минут.
- Astro-клиент использует `EventSource` в островке, без дополнительных библиотек.

## Consequences

- Один протокол (HTTP) на всё: проще меш, трассировка, rate limiting, запись Keploy.
- Если появится нужда в частом клиент→сервер обмене (чат на лоте, авто-ставки с клиента), откроем WebSocket отдельным ADR, SSE останется для цены.
- Нагрузочный профиль — много долгоживущих соединений. В Rust на tokio это дёшево, но лимиты файловых дескрипторов и `max_connections` Envoy нужно поднять осознанно.

## Sources

- [SSE vs WebSockets (SoftwareMill)](https://softwaremill.com/sse-vs-websockets-comparing-real-time-communication-protocols/)
- [SSE and why HTTP/2 matters](https://dev.to/abhivyaktii/understanding-server-sent-events-sse-and-why-http2-matters-1cj7)
- [Configure Istio for SSE](https://oneuptime.com/blog/post/2026-02-24-how-to-configure-istio-for-server-sent-events/view)
- [Axum SSE module](https://www.rustmax.net/api/axum/response/sse/)
- [Astro + SSE notifications](https://www.koyeb.com/tutorials/using-astro-and-server-sent-events-sse-to-build-realtime-in-app-notifications)
