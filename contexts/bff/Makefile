SERVICE := bff
include ../../ops/Makefiles/service.mk

.PHONY: graphql-check graphql-compose
graphql-check: ## проверить Federation composition
	cd $(ROOT_DIR) && npm run graphql:check
graphql-compose: ## сгенерировать execution config
	cd $(ROOT_DIR) && npm run graphql:compose
check: graphql-check
