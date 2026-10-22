# 16. Cloudflare as the mandatory public edge

Date: 2026-10-22

## Status

Accepted

## Context

Gavel публикует Web, federated GraphQL endpoint и callback платёжного провайдера. Открытый Kubernetes load balancer перед Istio оставляет origin доступным в обход WAF/DDoS controls и заставляет самостоятельно решать edge TLS, bot traffic и volumetric attacks.

Cloudflare должен находиться перед всей публичной границей, но не становиться местом бизнес-логики, authentication или authoritative authorization. GraphQL subscriptions создают долгоживущие соединения, которые edge может завершить при idle timeout или обновлении инфраструктуры, поэтому client reconnect и replay остаются обязательными.

## Decision

### Traffic path

- Использовать proxied Cloudflare DNS для публичных hostnames Gavel.
- Публичный путь: `Client/Provider → Cloudflare → Cloudflare Tunnel → Istio Ingress Gateway`.
- Запустить минимум две `cloudflared` replicas в разных availability zones; connector устанавливает исходящие tunnel connections.
- Не публиковать Istio Ingress через Internet-facing load balancer и не оставлять DNS-only origin hostname. Аварийный administrative access не является public data path.
- Cloudflare проксирует Web, `/graphql` и `/payment-provider-callbacks/*`; внутренние subgraphs и mesh endpoints не публикуются.

### Responsibility split

Cloudflare отвечает за:

- authoritative DNS и edge TLS;
- L3/L4 DDoS mitigation, WAF и bot controls;
- грубые anonymous/IP/ASN/geo rate limits;
- ограничение GraphQL query depth/size как дополнительную защиту, если доступно в выбранном plan;
- CDN cache только для immutable Web assets;
- передачу `CF-Ray` и проверенного client IP для observability и edge policies.

Istio, Cosmo и owner services сохраняют ответственность за:

- JWT verification и canonical principal;
- per-principal limits и overload/backpressure по известной операции;
- persisted-operation allowlist и GraphQL execution budgets;
- authoritative SpiceDB checks, идемпотентность и domain invariants.

Cloudflare allow или challenge никогда не считается authorization decision Gavel.

### Cache and route policy

- `/graphql`, authentication responses, personalized HTML и provider callbacks получают `Cache-Control: no-store` и bypass cache rules.
- Immutable versioned assets можно кэшировать на edge с длительным TTL.
- На payment callback route запрещены browser challenges и transformations; provider signature остаётся authoritative, callbacks дедуплицируются Payments.
- WAF и rate-limit rules для callback route тестируются на реальные retry/burst patterns провайдера.

### Subscriptions

- Выбранный Cosmo subscription transport должен быть явно включён и проверен через Cloudflare Tunnel.
- Для WebSocket используются heartbeat/ping-pong и reconnect; соединение не хранит незаменимое session state в конкретной Router replica.
- Любой edge disconnect считается нормальным failure mode: клиент переподписывается с `afterVersion`, Realtime subgraph делает replay и затем возвращается к live stream.
- k6 профиль включает Cloudflare path, reconnect storm, idle connection и terminal-event delivery.

## Consequences

- Публичный origin нельзя обойти по известному IP; весь пользовательский и provider traffic проходит одинаковый edge policy layer.
- Cloudflare становится внешней synchronous dependency и входит в end-to-end availability/error-budget model.
- Edge rate limiting снижает мусорную нагрузку, но identity-aware защита остаётся в Istio/Cosmo.
- Cache ускоряет Web assets, но не может скрывать stale auction/payment data.
- `CF-Ray`, tunnel status и Cloudflare security events становятся обязательными сигналами observability.
- Локальная разработка не требует Cloudflare; kind тестирует origin path, а отдельный integration/performance environment — полный edge path.

## References

- [Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/)
- [Cloudflare Tunnel WebSocket support](https://developers.cloudflare.com/cloudflare-one/faq/cloudflare-tunnels-faq/)
- [Cloudflare WebSockets](https://developers.cloudflare.com/network/websockets/)
- [GraphQL malicious query protection](https://developers.cloudflare.com/api-shield/security/graphql-protection/)
- [Rate limiting best practices](https://developers.cloudflare.com/waf/rate-limiting-rules/best-practices/)
- [BFF init](../../contexts/bff/adr/0001-init.md)
- [Availability ADR](0014-availability-replication-and-sharding.md)
