SERVICE := catalog
DOCKERFILE := ops/Dockerfile
DOCKER_CONTEXT := ../..
include ../../ops/Makefiles/service.mk

POSTGRES_IMAGE := postgres:17.6-alpine
POSTGRES_CONTAINER := gavel-catalog-postgres-test
POSTGRES_PORT ?= 55440
POSTGRES_PASSWORD := gavel_test
POSTGRES_DATABASE := gavel_catalog_test
CATALOG_TEST_DATABASE_URL := postgres://postgres:$(POSTGRES_PASSWORD)@127.0.0.1:$(POSTGRES_PORT)/$(POSTGRES_DATABASE)
MINIO_IMAGE := bitnamilegacy/minio:2025.4.22-debian-12-r1
MINIO_CONTAINER := gavel-catalog-minio-test
MINIO_PORT ?= 59000
MINIO_CONSOLE_PORT ?= 59001
MINIO_ACCESS_KEY := gavel_catalog_test
MINIO_SECRET_KEY := gavel_catalog_test_secret
CATALOG_TEST_S3_ENDPOINT := http://127.0.0.1:$(MINIO_PORT)

.PHONY: database-up database-down database-check storage-up storage-down storage-check integration-check observability-contract-check
observability-check: observability-contract-check
observability-contract-check: ## проверить соответствие runtime metrics, rules, dashboard и runbook
	bash ops/check-observability.sh
database-up: ## поднять pinned disposable PostgreSQL для Catalog integration tests
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

database-down: ## остановить disposable PostgreSQL Catalog
	@docker rm -f $(POSTGRES_CONTAINER) >/dev/null 2>&1 || true

database-check: ## прогнать Catalog PostgreSQL migrations и persistence integration tests
	@set -euo pipefail; \
		$(MAKE) --no-print-directory database-down; \
		trap '$(MAKE) --no-print-directory database-down' EXIT; \
		$(MAKE) --no-print-directory database-up; \
		CATALOG_TEST_DATABASE_URL='$(CATALOG_TEST_DATABASE_URL)' \
			cargo test --manifest-path Cargo.toml \
				--test postgres_migrations --test postgres_unit_of_work --test postgres_reader \
				--test postgres_repositories \
				-- --test-threads=1

storage-up: ## поднять pinned disposable MinIO для Catalog integration tests
	docker run --rm -d --name $(MINIO_CONTAINER) \
		-e MINIO_ROOT_USER=$(MINIO_ACCESS_KEY) \
		-e MINIO_ROOT_PASSWORD=$(MINIO_SECRET_KEY) \
		-p 127.0.0.1:$(MINIO_PORT):9000 \
		-p 127.0.0.1:$(MINIO_CONSOLE_PORT):9001 \
		$(MINIO_IMAGE)
	@for attempt in $$(seq 1 60); do \
		if curl --fail --silent '$(CATALOG_TEST_S3_ENDPOINT)/minio/health/ready' >/dev/null; then exit 0; fi; \
		sleep 1; \
	done; \
	echo "MinIO не стал ready за 60 секунд" >&2; \
	exit 1

storage-down: ## остановить disposable MinIO Catalog
	@docker rm -f $(MINIO_CONTAINER) >/dev/null 2>&1 || true

storage-check: ## прогнать реальный S3 boundary test на pinned MinIO
	@set -euo pipefail; \
		$(MAKE) --no-print-directory storage-down; \
		trap '$(MAKE) --no-print-directory storage-down' EXIT; \
		$(MAKE) --no-print-directory storage-up; \
		CATALOG_TEST_S3_ENDPOINT='$(CATALOG_TEST_S3_ENDPOINT)' \
		CATALOG_TEST_S3_ACCESS_KEY='$(MINIO_ACCESS_KEY)' \
		CATALOG_TEST_S3_SECRET_KEY='$(MINIO_SECRET_KEY)' \
			cargo test --manifest-path Cargo.toml \
				-p gavel-catalog minio_round_trip_verifies_metadata_mismatch_expiry_and_delete \
				-- --nocapture

integration-check: ## прогнать единый Catalog flow через PostgreSQL и MinIO
	@set -euo pipefail; \
		$(MAKE) --no-print-directory database-down; \
		$(MAKE) --no-print-directory storage-down; \
		trap '$(MAKE) --no-print-directory database-down; $(MAKE) --no-print-directory storage-down' EXIT; \
		$(MAKE) --no-print-directory database-up; \
		$(MAKE) --no-print-directory storage-up; \
		CATALOG_TEST_DATABASE_URL='$(CATALOG_TEST_DATABASE_URL)' \
		CATALOG_TEST_S3_ENDPOINT='$(CATALOG_TEST_S3_ENDPOINT)' \
		CATALOG_TEST_S3_ACCESS_KEY='$(MINIO_ACCESS_KEY)' \
		CATALOG_TEST_S3_SECRET_KEY='$(MINIO_SECRET_KEY)' \
			cargo test --manifest-path Cargo.toml \
				-p gavel-catalog --test catalog_boundary_flow -- --nocapture
