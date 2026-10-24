SERVICE := processes
DOCKER_CONTEXT := ../..
DOCKERFILE := contexts/processes/ops/Dockerfile
include ../../ops/Makefiles/service.mk

TEMPORAL_CLI_VERSION ?= v1.8.3

.PHONY: integration-check helm-render-check helm-local-render-check
integration-check: ## synthetic Temporal start → timer → activity → completion
	@mkdir -p target/temporal-cli-cache
	@mkdir -p target/temporal-test-server-cache
	TEMPORAL_CLI_VERSION=$(TEMPORAL_CLI_VERSION) cargo test --tests -- --ignored --nocapture --test-threads=1

helm-check: helm-render-check helm-local-render-check
helm-render-check: ## проверить production render assertions
	@rendered="$$(mktemp)"; status=0; helm template processes ops/Helm >"$$rendered" || status=1; \
		grep -q 'replicas: 2' "$$rendered" || status=1; \
		grep -q 'kind: PodDisruptionBudget' "$$rendered" || status=1; \
		grep -q 'topologySpreadConstraints:' "$$rendered" || status=1; \
		grep -q 'name: TEMPORAL_API_KEY' "$$rendered" || status=1; \
		grep -q 'name: REDPANDA_BROKERS' "$$rendered" || status=1; \
		grep -q 'secretKeyRef:' "$$rendered" || status=1; rm -f "$$rendered"; exit $$status

helm-local-render-check: ## проверить local broker/RPC/readiness wiring
	@rendered="$$(mktemp)"; status=0; helm template gavel-processes ops/Helm -f ops/Helm/values-local.yaml >"$$rendered" || status=1; \
		grep -q 'name: REDPANDA_BROKERS' "$$rendered" || status=1; \
		grep -q 'name: REDPANDA_SASL_USERNAME' "$$rendered" || status=1; \
		grep -q 'name: AUCTION_GRPC_ENDPOINT' "$$rendered" || status=1; \
		grep -q 'name: PAYMENTS_GRPC_ENDPOINT' "$$rendered" || status=1; \
		grep -q 'name: NOTIFY_GRPC_ENDPOINT' "$$rendered" || status=1; rm -f "$$rendered"; exit $$status
