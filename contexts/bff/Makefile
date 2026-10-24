SERVICE := bff
DOCKERFILE := contexts/bff/ops/Dockerfile
DOCKER_CONTEXT := ../..
include ../../ops/Makefiles/service.mk

.PHONY: graphql-check graphql-compose graphql-fixtures-check helm-local-check
helm-check: helm-local-check
helm-local-check: ## проверить local JWT ingress overlay и production guard
	helm lint ops/Helm -f ops/Helm/values-local.yaml
	bash ops/Helm/tests/assert-render.sh
graphql-check: graphql-fixtures-check ## проверить Federation composition и MVP operations
	cd $(ROOT_DIR) && npm run graphql:operations:check
graphql-compose: ## сгенерировать execution config
	cd $(ROOT_DIR) && npm run graphql:compose
graphql-fixtures-check: ## проверить pinned Router runtime и BFF contract fixtures
	cd $(ROOT_DIR) && node contexts/bff/scripts/check-fixtures.mjs
check: graphql-check
