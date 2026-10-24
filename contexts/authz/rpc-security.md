# RPC security boundary

Внутренний gRPC port `9090` публикуется только внутри mesh. Istio проверяет mTLS SPIFFE principal до передачи запроса приложению, а Kubernetes NetworkPolicy ограничивает источники известными owner workloads.

Authz inbound boundary обязан:

1. получить verified workload identity из trusted proxy context, а не из произвольного application header;
2. принять metadata только от loopback/настроенного trusted proxy CIDR и сопоставить SPIFFE service account с закрытым enum;
3. разрешить `RegisterPrincipal` BFF; отдельной bootstrap workload identity разрешить регистрацию local fixtures и единственный фиксированный grant `platform:gavel#process_worker@service_account:processes`; остальные relationship mutation — только владельцу resource/relation согласно compile-time allowlist;
4. провалидировать весь batch и overlap TOUCH/delete до receipt claim и SpiceDB;
5. fail closed при timeout или ошибке receipt store/SpiceDB;
6. не логировать principal/resource ids, issuer/subject, ZedToken, credentials или idempotency key.

Default SPIFFE service accounts: `gavel-bff`, `gavel-auction`, `gavel-catalog`, `gavel-payments`, `gavel-notify`, `gavel-processes`. `gavel-authz-bootstrap` существует только при явно включённом local Helm profile. Raw `x-workload`, `x-principal-id` и похожие client headers никогда не определяют caller. В local kind verified identity metadata принимается только из pinned pod CIDR вместе с NetworkPolicy; production обязан использовать mesh proxy. Dev/test injector запускается только явным test-only конструктором и отсутствует в Helm values.

Только Authz adapter получает SpiceDB endpoint/credential, PostgreSQL receipt URL и HMAC keys из external Secrets. Остальные проекты используют [`gavel.authz.v1.AuthzService`](proto/gavel/authz/v1/authz_service.proto); raw SpiceDB API им не доступен. Causal checks передают `written_at_token` как `at_least_as_fresh`; token не является identity или permission proof и не должен попадать в логи.

Default principals в Helm values предназначены для namespace `gavel`. При другом trust domain или namespace production values обязаны переопределить полный список SPIFFE principals.
