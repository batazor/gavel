.DEFAULT_GOAL := help
ROOT_DIR := $(abspath ../..)

.PHONY: image helm-check help
image: ## собрать local Web image
	cd $(ROOT_DIR) && docker build -f packages/web/ops/Dockerfile -t gavel-web:local .

helm-check: ## проверить Web chart и local overlay
	cd $(ROOT_DIR) && helm lint packages/web/ops/Helm -f packages/web/ops/Helm/values-local.yaml
	cd $(ROOT_DIR) && helm template gavel-web packages/web/ops/Helm -f packages/web/ops/Helm/values-local.yaml >/dev/null
	cd $(ROOT_DIR) && bash packages/web/ops/Helm/tests/assert-render.sh

help: ## показать цели Web delivery
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}'
