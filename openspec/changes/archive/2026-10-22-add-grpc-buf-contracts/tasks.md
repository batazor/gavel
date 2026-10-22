# Tasks

## 1. Shared architecture and Buf policy

- [x] 1.1 Add the global ADR “Internal RPC with gRPC and Buf”, index it in `docs/adr/README.md`, and verify every transport/registry boundary from the design is explicit by reviewing the rendered ADR links.
- [x] 1.2 Add root Buf v2 workspace configuration, generation configuration and a `buf-check` Make target covering format, lint and build; verify `make help` lists it and `make buf-check` passes.
- [x] 1.3 Add a baseline-aware `buf-breaking` target and CI contract step that compares against the default branch when available; verify a compatible branch passes and a temporary removed field is rejected by `buf breaking`.

## 2. Owner RPC modules

- [x] 2.1 Add `gavel.auction.v1.AuctionService` under `contexts/auction/proto` with CloseAuction, CancelAuction, ForfeitSettlementCandidate and ConfirmWinner contracts, including idempotency and typed results; verify `buf lint` and `buf build` pass for the workspace.
- [x] 2.2 Add `gavel.catalog.v1.CatalogService` under `contexts/catalog/proto` with WithdrawLot contract and idempotency semantics; verify `buf lint` and `buf build` pass.
- [x] 2.3 Add `gavel.payments.v1.PaymentsService` under `contexts/payments/proto` with provider-neutral AttemptPayment and ExpirePayment contracts that expose no provider objects; verify `buf lint` and `buf build` pass.
- [x] 2.4 Add `gavel.notify.v1.NotifyService` under `contexts/notify/proto` with provider-neutral SendNotification contract; verify `buf lint` and `buf build` pass.
- [x] 2.5 Add `gavel.authz.v1.AuthzService` under `contexts/authz/proto` with CheckPermission, BulkCheckPermissions and caller-scoped WriteRelationships contracts; verify `buf lint`, `buf build` and existing `make authz-validate` pass.
- [x] 2.6 Add pinned Rust stub generation/compile wiring for each owner module without committing generated output, and verify `make fmt-check lint test` passes for Auction, Catalog, Payments, Notify and Authz.

## 3. Consumers and runtime boundary

- [x] 3.1 Wire Processes to compile clients from owner modules and document deadline, retry, idempotency-key and gRPC status mapping used by Temporal activities; verify `make -C contexts/processes fmt-check lint test` passes.
- [x] 3.2 Document and configure the owner-side mTLS workload identity/Authz interception boundary without exposing raw SpiceDB credentials; verify Authz schema assertions and Helm render/network policy checks pass.
- [x] 3.3 Add bounded-label gRPC client/server metrics and trace propagation expectations to each affected service-owned observability README, dashboard and Prometheus rules; verify every affected `make -C contexts/<service> observability-check` passes.

## 4. Service documentation and architecture views

- [x] 4.1 Update Auction, Catalog, Payments, Notify, Authz and Processes init ADR/README files with owned or consumed RPC contracts and links; verify each README links its local ADR and proto source.
- [x] 4.2 Update BFF init ADR/README to state that Cosmo-to-subgraph traffic remains GraphQL/HTTP and internal RPC endpoints are not public; verify `make graphql-check` passes with no unnecessary Federation SDL change.
- [x] 4.3 Update affected use-case Mermaid sequences and matching `flow.c4` relations to label synchronous calls as gRPC while retaining Redpanda event arrows; verify `make arch-validate` passes and diagrams agree for close, withdraw, settlement and authorization flows.
- [x] 4.4 Update `VISION.md` to separate the resolved RPC/Buf decision from the still-open event format/Redpanda Schema Registry decision; verify the two registries cannot be read as interchangeable.

## 5. Integration verification

- [x] 5.1 Run `make openspec-validate`, `make buf-check`, `make buf-breaking`, `make graphql-check`, `make authz-validate` and `make arch-validate`, then record any environment-dependent unavailable check before marking the change complete.
- [x] 5.2 Run the affected service checks discoverable through `make help` and `make service-help SERVICE=<name>` for Auction, Catalog, Payments, Notify, Authz, Processes and BFF; verify no generated files or provider-specific secrets are committed.

## Workflow follow-up

- После реализации и review синхронизировать capability spec и архивировать change.
- BSR publication/deployment и выбор Protobuf для Redpanda events оформить отдельными changes после появления первого независимого consumer release cycle.

## Verification note

- `make buf-breaking` 2026-10-22 корректно пропустил repository baseline: `origin/main` ещё не содержит `buf.yaml` и `.proto`. Механизм отдельно проверен локальным descriptor snapshot: удаление `Money.currency_code` было отклонено Buf как удаление ранее существовавшего field 2. После первого merge target автоматически начнёт сравнивать последующие changes с `origin/main`.
