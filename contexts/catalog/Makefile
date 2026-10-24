SERVICE := catalog
DOCKERFILE := ops/Dockerfile
DOCKER_CONTEXT := ../..
include ../../ops/Makefiles/service.mk

.PHONY: helm-render-check
helm-check: helm-render-check
helm-render-check: ## проверить default/local render и обязательный runtime contract Catalog
	helm template gavel-catalog ops/Helm | ops/Helm/tests/assert-render.sh
	helm template gavel-catalog ops/Helm -f ops/Helm/values-local.yaml | ops/Helm/tests/assert-render.sh

POSTGRES_IMAGE := postgres:17.6-alpine
POSTGRES_CONTAINER := gavel-catalog-postgres-test
POSTGRES_PORT ?= 55440
POSTGRES_PASSWORD := gavel_test
POSTGRES_DATABASE := gavel_catalog_test
CATALOG_TEST_DATABASE_URL := postgres://postgres:$(POSTGRES_PASSWORD)@127.0.0.1:$(POSTGRES_PORT)/$(POSTGRES_DATABASE)
# s3s-fs v0.17.0 multi-platform manifest.
S3S_IMAGE := ghcr.io/s3s-project/s3s@sha256:565e16c104805fa9d129f22134ea12d309a634f57f73185a32b3ac4b7c38b049
S3S_CONTAINER := gavel-catalog-s3s-test
S3S_PORT ?= 59000
S3S_ACCESS_KEY := gavel_catalog_test
S3S_SECRET_KEY := gavel_catalog_test_secret
CATALOG_TEST_S3_ENDPOINT := http://127.0.0.1:$(S3S_PORT)

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

storage-up: ## поднять pinned disposable s3s-fs для Catalog integration tests
	docker run --rm -d --name $(S3S_CONTAINER) \
		-p 127.0.0.1:$(S3S_PORT):8014 \
		$(S3S_IMAGE) ./s3s-fs \
			--host 0.0.0.0 --port 8014 \
			--access-key $(S3S_ACCESS_KEY) --secret-key $(S3S_SECRET_KEY) \
			/data
	@for attempt in $$(seq 1 60); do \
		if curl --silent --output /dev/null --connect-timeout 1 --max-time 1 \
			'$(CATALOG_TEST_S3_ENDPOINT)/'; then exit 0; fi; \
		sleep 1; \
	done; \
	echo "s3s-fs не стал ready за 60 секунд" >&2; \
	exit 1

storage-down: ## остановить disposable s3s-fs Catalog
	@docker rm -f $(S3S_CONTAINER) >/dev/null 2>&1 || true

storage-check: ## прогнать реальный S3 boundary test на pinned s3s-fs
	@set -euo pipefail; \
		$(MAKE) --no-print-directory storage-down; \
		trap '$(MAKE) --no-print-directory storage-down' EXIT; \
		$(MAKE) --no-print-directory storage-up; \
		CATALOG_TEST_S3_ENDPOINT='$(CATALOG_TEST_S3_ENDPOINT)' \
		CATALOG_TEST_S3_ACCESS_KEY='$(S3S_ACCESS_KEY)' \
		CATALOG_TEST_S3_SECRET_KEY='$(S3S_SECRET_KEY)' \
			cargo test --manifest-path Cargo.toml \
				-p gavel-catalog s3s_round_trip_verifies_metadata_mismatch_expiry_and_delete \
				-- --nocapture

integration-check: ## прогнать единый Catalog flow через PostgreSQL и s3s-fs
	@set -euo pipefail; \
		$(MAKE) --no-print-directory database-down; \
		$(MAKE) --no-print-directory storage-down; \
		trap '$(MAKE) --no-print-directory database-down; $(MAKE) --no-print-directory storage-down' EXIT; \
		$(MAKE) --no-print-directory database-up; \
		$(MAKE) --no-print-directory storage-up; \
		CATALOG_TEST_DATABASE_URL='$(CATALOG_TEST_DATABASE_URL)' \
		CATALOG_TEST_S3_ENDPOINT='$(CATALOG_TEST_S3_ENDPOINT)' \
		CATALOG_TEST_S3_ACCESS_KEY='$(S3S_ACCESS_KEY)' \
		CATALOG_TEST_S3_SECRET_KEY='$(S3S_SECRET_KEY)' \
			cargo test --manifest-path Cargo.toml \
				-p gavel-catalog --test catalog_boundary_flow -- --nocapture
