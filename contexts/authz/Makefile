SERVICE := authz
DOCKER_CONTEXT := ../..
include ../../ops/Makefiles/service.mk

POSTGRES_IMAGE := postgres:17.6-alpine
POSTGRES_CONTAINER := gavel-authz-postgres-test
POSTGRES_PORT ?= 55440
POSTGRES_PASSWORD := gavel_test
POSTGRES_DATABASE := gavel_authz_test
AUTHZ_TEST_DATABASE_URL := postgres://postgres:$(POSTGRES_PASSWORD)@127.0.0.1:$(POSTGRES_PORT)/$(POSTGRES_DATABASE)
SPICEDB_IMAGE := ghcr.io/authzed/spicedb:v1.56.2@sha256:dd98bfca12f111a0e698bf348723fe4e443d2189a6bd40481adb31ee9789a6ad
SPICEDB_CONTAINER := gavel-authz-spicedb-test
SPICEDB_PORT ?= 55051
SPICEDB_TOKEN := gavel_authz_test_token
SPICEDB_ENDPOINT := 127.0.0.1:$(SPICEDB_PORT)

.PHONY: database-up database-down database-check
database-up: ## поднять pinned disposable PostgreSQL для Authz tests
	docker run --rm -d --name $(POSTGRES_CONTAINER) \
		-e POSTGRES_PASSWORD=$(POSTGRES_PASSWORD) \
		-e POSTGRES_DB=$(POSTGRES_DATABASE) \
		-p 127.0.0.1:$(POSTGRES_PORT):5432 $(POSTGRES_IMAGE)
	@for attempt in $$(seq 1 30); do \
		if docker exec $(POSTGRES_CONTAINER) pg_isready -U postgres -d $(POSTGRES_DATABASE) >/dev/null 2>&1; then exit 0; fi; \
		sleep 1; \
	done; \
	echo "PostgreSQL не стал ready за 30 секунд" >&2; \
	exit 1

database-down: ## остановить disposable Authz PostgreSQL
	@docker rm -f $(POSTGRES_CONTAINER) >/dev/null 2>&1 || true

database-check: ## проверить Authz migrations на disposable PostgreSQL
	@set -euo pipefail; \
		$(MAKE) --no-print-directory database-down; \
		trap '$(MAKE) --no-print-directory database-down' EXIT; \
		$(MAKE) --no-print-directory database-up; \
		AUTHZ_TEST_DATABASE_URL='$(AUTHZ_TEST_DATABASE_URL)' \
			cargo test --manifest-path Cargo.toml \
				--test postgres_migrations --test postgres_receipts -- --test-threads=1

.PHONY: spicedb-up spicedb-down spicedb-check
spicedb-up: ## поднять pinned disposable SpiceDB с in-memory datastore
	docker run --rm -d --name $(SPICEDB_CONTAINER) \
		-p 127.0.0.1:$(SPICEDB_PORT):50051 \
		$(SPICEDB_IMAGE) serve \
			--grpc-preshared-key=$(SPICEDB_TOKEN) \
			--datastore-engine=memory \
			--skip-release-check
	@for attempt in $$(seq 1 30); do \
		if nc -z 127.0.0.1 $(SPICEDB_PORT) >/dev/null 2>&1; then exit 0; fi; \
		sleep 1; \
	done; \
	docker logs $(SPICEDB_CONTAINER) >&2 || true; \
	echo "SpiceDB не стал ready за 30 секунд" >&2; \
	exit 1

spicedb-down: ## остановить disposable Authz SpiceDB
	@docker rm -f $(SPICEDB_CONTAINER) >/dev/null 2>&1 || true

spicedb-check: ## загрузить production schema и проверить SpiceDB adapter suite
	@set -euo pipefail; \
		$(MAKE) --no-print-directory spicedb-down; \
		trap '$(MAKE) --no-print-directory spicedb-down' EXIT; \
		$(MAKE) --no-print-directory spicedb-up; \
		go run github.com/authzed/zed/cmd/zed@v0.36.0 schema write schema/gavel.zed \
			--endpoint '$(SPICEDB_ENDPOINT)' --token '$(SPICEDB_TOKEN)' --insecure; \
		cargo test --manifest-path Cargo.toml --test spicedb_adapter -- --test-threads=1

.PHONY: integration-up integration-down integration-check
integration-up: database-up spicedb-up ## поднять Authz PostgreSQL и SpiceDB

integration-down: spicedb-down database-down ## остановить Authz integration dependencies

integration-check: ## прогнать Authz persistence и SpiceDB suites одной командой
	@set -euo pipefail; \
		$(MAKE) --no-print-directory integration-down; \
		trap '$(MAKE) --no-print-directory integration-down' EXIT; \
		$(MAKE) --no-print-directory integration-up; \
		go run github.com/authzed/zed/cmd/zed@v0.36.0 schema write schema/gavel.zed \
			--endpoint '$(SPICEDB_ENDPOINT)' --token '$(SPICEDB_TOKEN)' --insecure; \
		AUTHZ_TEST_DATABASE_URL='$(AUTHZ_TEST_DATABASE_URL)' \
		AUTHZ_TEST_SPICEDB_ENDPOINT='http://$(SPICEDB_ENDPOINT)' \
		AUTHZ_TEST_SPICEDB_TOKEN='$(SPICEDB_TOKEN)' \
			cargo test --manifest-path Cargo.toml \
				--test postgres_migrations --test postgres_receipts \
				--test spicedb_adapter --test real_authz_flow -- --test-threads=1

.PHONY: schema-validate
schema-validate: ## проверить SpiceDB schema и assertions
	@command -v zed >/dev/null 2>&1 && zed validate schema/gavel.validation.yaml || \
		go run github.com/authzed/zed/cmd/zed@v0.36.0 validate schema/gavel.validation.yaml

.PHONY: spicedb-proto-check
spicedb-proto-check: ## проверить pinned Authzed API subset и offline Rust generation
	@test -f proto/authzed/LICENSE
	@grep -q '3270b08244f3e97e2a57a7ee2d9c649c88b861d3' proto/authzed/UPSTREAM.md
	cargo check --manifest-path Cargo.toml --offline
check: schema-validate

.PHONY: observability-contract-check
observability-check: observability-contract-check
observability-contract-check: ## проверить согласованность Authz metrics, rules, dashboard и runbook
	@bash ops/check-observability.sh
