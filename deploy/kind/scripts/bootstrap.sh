#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"
DRY_RUN=false
[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=true
MVP_BOOTSTRAP_TIMEOUT="${MVP_BOOTSTRAP_TIMEOUT:-600s}"

if ${DRY_RUN}; then
  "${KIND_DIR}/scripts/images.sh" --dry-run
  "${KIND_DIR}/scripts/secrets.sh" --dry-run
  "${KIND_DIR}/scripts/topics.sh" --dry-run
  "${KIND_DIR}/scripts/connectors.sh" --dry-run
  "${KIND_DIR}/scripts/migrations.sh" --dry-run
  "${KIND_DIR}/scripts/identity.sh" --dry-run
  exit 0
fi

"${KIND_DIR}/scripts/images.sh"
"${KIND_DIR}/scripts/secrets.sh"
kubectl --context "${MVP_CONTEXT}" apply -k "${KIND_DIR}"
kubectl_mvp wait --for=condition=complete job/postgres-bootstrap --timeout="${MVP_BOOTSTRAP_TIMEOUT}"
kubectl_mvp wait --for=condition=complete job/temporal-namespace-bootstrap --timeout="${MVP_BOOTSTRAP_TIMEOUT}"
kubectl_mvp rollout status deployment/redis --timeout="${MVP_BOOTSTRAP_TIMEOUT}"
kubectl_mvp rollout status statefulset/redpanda --timeout="${MVP_BOOTSTRAP_TIMEOUT}"
"${KIND_DIR}/scripts/topics.sh"
kubectl_mvp rollout status deployment/kafka-connect --timeout="${MVP_BOOTSTRAP_TIMEOUT}"
kubectl_mvp rollout status deployment/spicedb --timeout="${MVP_BOOTSTRAP_TIMEOUT}"
kubectl_mvp rollout status deployment/local-jwt --timeout="${MVP_BOOTSTRAP_TIMEOUT}"
"${KIND_DIR}/scripts/migrations.sh"
"${KIND_DIR}/scripts/identity.sh"
"${KIND_DIR}/scripts/connectors.sh"
