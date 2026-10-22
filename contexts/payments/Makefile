SERVICE := payments
DOCKER_CONTEXT := ../..
DOCKERFILE := ops/Dockerfile
include ../../ops/Makefiles/service.mk

POSTGRES_IMAGE := postgres:17.6-alpine3.22
POSTGRES_CONTAINER := gavel-payments-postgres
TEST_DATABASE_URL := postgres://postgres:postgres@127.0.0.1:55432/payments

.PHONY: database-up database-down database-check integration-check
database-up:
	docker run -d --rm --name $(POSTGRES_CONTAINER) -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=payments -p 55432:5432 $(POSTGRES_IMAGE)

database-down:
	@docker rm -f $(POSTGRES_CONTAINER) >/dev/null 2>&1 || true

database-check:
	@set -euo pipefail; $(MAKE) --no-print-directory database-down; trap '$(MAKE) --no-print-directory database-down' EXIT; $(MAKE) --no-print-directory database-up; until docker exec $(POSTGRES_CONTAINER) pg_isready -U postgres -d payments >/dev/null 2>&1; do sleep 1; done; DATABASE_URL=$(TEST_DATABASE_URL) cargo test --test postgres_integration -- --ignored

integration-check:
	@set -euo pipefail; $(MAKE) --no-print-directory database-down; trap '$(MAKE) --no-print-directory database-down' EXIT; $(MAKE) --no-print-directory database-up; until docker exec $(POSTGRES_CONTAINER) pg_isready -U postgres -d payments >/dev/null 2>&1; do sleep 1; done; DATABASE_URL=$(TEST_DATABASE_URL) cargo test --test payment_flows -- --ignored
