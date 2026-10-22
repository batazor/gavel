SERVICE := auction
include ../../ops/Makefiles/service.mk

POSTGRES_IMAGE := postgres:17.6-alpine
POSTGRES_CONTAINER := gavel-auction-postgres-test
POSTGRES_PORT ?= 55439
POSTGRES_PASSWORD := gavel_test
POSTGRES_DATABASE := gavel_auction_test
AUCTION_TEST_DATABASE_URL := postgres://postgres:$(POSTGRES_PASSWORD)@127.0.0.1:$(POSTGRES_PORT)/$(POSTGRES_DATABASE)
REDIS_IMAGE := redis:7.4.7-alpine
REDIS_CONTAINER := gavel-auction-redis-test
REDIS_PORT ?= 56379
AUCTION_TEST_REDIS_URL := redis://127.0.0.1:$(REDIS_PORT)/

.PHONY: database-up database-down database-check
database-up: ## поднять pinned disposable PostgreSQL для integration tests
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

database-down: ## остановить disposable PostgreSQL
	@docker rm -f $(POSTGRES_CONTAINER) >/dev/null 2>&1 || true

database-check: ## прогнать PostgreSQL migrations и repository integration tests
	@set -euo pipefail; \
		$(MAKE) --no-print-directory database-down; \
		trap '$(MAKE) --no-print-directory database-down' EXIT; \
		$(MAKE) --no-print-directory database-up; \
		AUCTION_TEST_DATABASE_URL='$(AUCTION_TEST_DATABASE_URL)' \
			cargo test --manifest-path Cargo.toml \
				--test postgres_migrations --test postgres_unit_of_work -- --test-threads=1

.PHONY: cache-up cache-down cache-check
cache-up: ## поднять pinned disposable Redis для cache integration tests
	docker run --rm -d --name $(REDIS_CONTAINER) \
		-p 127.0.0.1:$(REDIS_PORT):6379 $(REDIS_IMAGE)
	@for attempt in $$(seq 1 30); do \
		if docker exec $(REDIS_CONTAINER) redis-cli ping 2>/dev/null | grep -q PONG; then exit 0; fi; \
		sleep 1; \
	done; \
	echo "Redis не стал ready за 30 секунд" >&2; \
	exit 1

cache-down: ## остановить disposable Redis
	@docker rm -f $(REDIS_CONTAINER) >/dev/null 2>&1 || true

cache-check: ## прогнать Redis Lua/projector integration и cache privacy tests
	@set -euo pipefail; \
		$(MAKE) --no-print-directory cache-down; \
		trap '$(MAKE) --no-print-directory cache-down' EXIT; \
		$(MAKE) --no-print-directory cache-up; \
		AUCTION_TEST_REDIS_URL='$(AUCTION_TEST_REDIS_URL)' \
			cargo test --manifest-path Cargo.toml \
				--test redis_cache --test public_event_contract -- --test-threads=1

.PHONY: integration-up integration-down integration-check
integration-up: database-up cache-up ## поднять PostgreSQL и Redis для end-to-end tests

integration-down: cache-down database-down ## остановить end-to-end dependencies

integration-check: ## прогнать полный Auction flow на disposable PostgreSQL и Redis
	@set -euo pipefail; \
		$(MAKE) --no-print-directory integration-down; \
		trap '$(MAKE) --no-print-directory integration-down' EXIT; \
		$(MAKE) --no-print-directory integration-up; \
		AUCTION_TEST_DATABASE_URL='$(AUCTION_TEST_DATABASE_URL)' \
		AUCTION_TEST_REDIS_URL='$(AUCTION_TEST_REDIS_URL)' \
			cargo test --manifest-path Cargo.toml --test e2e_auction_flow -- --test-threads=1
