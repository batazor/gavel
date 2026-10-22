# Общая реализация стандартного interface сервисных Makefile.
ifndef SERVICE
$(error SERVICE must be set before including ops/Makefiles/service.mk)
endif

.DEFAULT_GOAL := help
SHELL := /bin/bash
PACKAGE ?= gavel-$(SERVICE)
IMAGE ?= ghcr.io/batazor/gavel-$(SERVICE):dev
DOCKERFILE ?= ops/Dockerfile
DOCKER_CONTEXT ?= .
ROOT_DIR := $(abspath $(CURDIR)/../..)
RULE_FILES := $(wildcard ops/Helm/files/prometheus/*.yaml)

.PHONY: fmt fmt-check lint test image helm-check observability-check check help
fmt: ## Rust: форматировать crate
	cargo fmt --manifest-path Cargo.toml
fmt-check: ## Rust: проверить форматирование crate
	cargo fmt --manifest-path Cargo.toml --check
lint: ## Rust: clippy для crate
	cargo clippy --manifest-path Cargo.toml --all-targets -- -D warnings
test: ## Rust: тесты crate
	@if command -v cargo-nextest >/dev/null 2>&1; then cargo nextest run --manifest-path Cargo.toml; else cargo test --manifest-path Cargo.toml; fi
image: ## собрать container image
	docker build -f $(DOCKERFILE) -t $(IMAGE) $(DOCKER_CONTEXT)
helm-check: ## проверить service-owned Helm chart
	helm lint ops/Helm
observability-check: ## проверить Prometheus rules и Grafana dashboards
	@for dashboard in ops/Helm/files/grafana/*.json; do jq empty $$dashboard; done
	@if command -v promtool >/dev/null 2>&1; then \
		promtool check rules $(RULE_FILES); \
	else \
		docker run --rm --entrypoint /bin/promtool -v "$(CURDIR):/workspace:ro" prom/prometheus:v3.13.4 check rules $(patsubst %,/workspace/%,$(RULE_FILES)); \
	fi
check: fmt-check lint test ## локальные проверки сервиса
help: ## показать цели сервиса
	@grep -h -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-22s\033[0m %s\n", $$1, $$2}'
