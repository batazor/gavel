# Gavel monorepo — агрегатор. Service-owned цели реализованы в contexts/*/Makefile.
.DEFAULT_GOAL := help
SHELL := /bin/bash
SERVICES := analytics auction authz bff catalog notify payments processes

define run_for_services
	@set -e; for service in $(SERVICES); do \
		$(MAKE) --no-print-directory -C contexts/$$service $(1); \
	done
endef

define require_service
	@test -n "$(SERVICE)" || (echo "Укажите SERVICE: $(SERVICES)" >&2; exit 2)
	@echo "$(SERVICES)" | tr ' ' '\n' | grep -Fx "$(SERVICE)" >/dev/null || \
		(echo "Неизвестный SERVICE=$(SERVICE). Допустимо: $(SERVICES)" >&2; exit 2)
endef

## OpenSpec, contracts и архитектура
.PHONY: openspec-status openspec-validate openspec-doctor contract-check buf-check buf-breaking buf-generate arch arch-validate arch-build arch-fmt
openspec-status: ## OpenSpec: показать активные changes
	openspec status
openspec-validate: ## OpenSpec: проверить все specs и changes
	openspec validate --all
openspec-doctor: ## OpenSpec: проверить root и stores/references
	openspec doctor
contract-check: ## Contracts: проверить event registry, privacy ACL и exact Buf/BSR pins
	node contracts/check-contracts.mjs
buf-check: ## Protobuf: проверить формат, lint и сборку Buf workspace
	buf format --diff --exit-code
	buf lint
	buf build
BUF_BREAKING_BASE ?= origin/main
buf-breaking: ## Protobuf: проверить обратную совместимость с доступной baseline-веткой
	@if git cat-file -e "$(BUF_BREAKING_BASE):buf.yaml" 2>/dev/null && \
		git ls-tree -r --name-only "$(BUF_BREAKING_BASE)" | grep -q '\.proto$$'; then \
		base_sha=$$(git rev-parse "$(BUF_BREAKING_BASE)"); \
		buf breaking --against ".git#ref=$$base_sha"; \
	else \
		echo "Buf baseline $(BUF_BREAKING_BASE) ещё не содержит contracts; breaking check пропущен"; \
	fi
buf-generate: ## Protobuf: сгенерировать Rust stubs в target/buf
	buf generate
arch: ## LikeC4: живой просмотр на http://localhost:5173
	npx likec4 start .
arch-validate: ## LikeC4: проверить модель и flow.c4
	npx likec4 validate .
arch-build: ## LikeC4: собрать статический сайт
	npx likec4 build . -o site --title "Gavel — архитектура"
arch-fmt: ## LikeC4: отформатировать .c4
	npx likec4 fmt .

## Сервисы
.PHONY: service-help fmt fmt-check lint test image helm-check service-observability-check services-check graphql-check graphql-compose authz-validate
service-help: ## показать цели сервиса: make service-help SERVICE=auction
	$(call require_service)
	@$(MAKE) --no-print-directory -C contexts/$(SERVICE) help
fmt: ## fmt всех сервисов
	$(call run_for_services,fmt)
fmt-check: ## проверить форматирование всех сервисов
	$(call run_for_services,fmt-check)
lint: ## clippy всех сервисов
	$(call run_for_services,lint)
test: ## тесты всех сервисов
	$(call run_for_services,test)
image: ## собрать image: make image SERVICE=auction
	$(call require_service)
	@$(MAKE) --no-print-directory -C contexts/$(SERVICE) image
helm-check: platform-helm-check web-helm-check ## platform, Web и Helm chart каждого сервиса
	$(call run_for_services,helm-check)
service-observability-check: ## service-owned Prometheus rules и Grafana dashboards
	$(call run_for_services,observability-check)
services-check: ## локальный check каждого сервиса
	$(call run_for_services,check)
graphql-check: ## Federation composition через Makefile BFF
	@$(MAKE) --no-print-directory -C contexts/bff graphql-check
graphql-compose: ## execution config через Makefile BFF
	@$(MAKE) --no-print-directory -C contexts/bff graphql-compose
authz-validate: ## SpiceDB schema через Makefile Authz
	@$(MAKE) --no-print-directory -C contexts/authz schema-validate

## Общая платформа
.PHONY: platform-helm-check web-helm-check observability-check observability-up observability-down lean kind-up kind-down mvp-platform mvp-up mvp-down mvp-seed mvp-ready mvp-smoke mvp-e2e mvp-diagnostics mvp-command-check
platform-helm-check: ## общий observability Helm chart
	helm repo add prometheus-community https://prometheus-community.github.io/helm-charts --force-update
	helm dependency build deploy/charts/observability
	helm lint deploy/charts/observability -f deploy/charts/observability/values-production.yaml
web-helm-check: ## Web chart, local image/ports и render assertions
	$(MAKE) --no-print-directory -C packages/web helm-check
observability-check: helm-check service-observability-check ## platform и service-owned observability
	docker compose -f deploy/compose/observability/compose.yaml config --quiet
observability-up: ## поднять локальный observability stack
	docker compose -f deploy/compose/observability/compose.yaml up -d
observability-down: ## остановить локальный observability stack
	docker compose -f deploy/compose/observability/compose.yaml down
lean: ## lake build в proofs/
	@[ -d proofs ] && (cd proofs && lake build) || echo "proofs/ нет, пропуск"
mvp-platform: ## MVP: создать kind и последовательно установить cluster platform
	@deploy/kind/scripts/mvp.sh platform
mvp-up: ## MVP: создать/сверить cluster gavel-mvp, bootstrap platform и дождаться readiness
	@deploy/kind/scripts/mvp.sh up
mvp-down: ## MVP: удалить только выделенный kind cluster gavel-mvp
	@deploy/kind/scripts/mvp.sh down
mvp-seed: ## MVP: идемпотентно подготовить local actors и короткий аукцион
	@deploy/kind/scripts/mvp.sh seed
mvp-ready: ## MVP: проверить готовность полного пользовательского пути
	@deploy/kind/scripts/mvp.sh ready
mvp-smoke: ## MVP: выполнить единый пакет трёх сквозных сценариев
	@deploy/kind/scripts/mvp.sh smoke
mvp-e2e: ## MVP: пройти happy path через настоящий Web UI, Temporal и Mailpit
	@deploy/kind/scripts/mvp.sh e2e
mvp-diagnostics: ## MVP: собрать ограниченный sanitized diagnostic bundle
	@deploy/kind/scripts/mvp.sh diagnostics
mvp-command-check: ## MVP: статически проверить command inventory и безопасные targets
	@deploy/kind/tests/command-inventory.sh
kind-up: mvp-up ## совместимый alias для mvp-up
kind-down: mvp-down ## совместимый alias для mvp-down

## Всё перед PR
.PHONY: check help
check: openspec-validate arch-validate services-check lean ## дешёвые monorepo checks
help: ## показать этот список
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-28s\033[0m %s\n", $$1, $$2}'
