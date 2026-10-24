#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"
DRY_RUN=false
[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=true
require_command helm

owners=(authz catalog auction payments notify processes bff web)
for owner in "${owners[@]}"; do
  if [[ "${owner}" == "web" ]]; then
    chart="${REPO_ROOT}/packages/web/ops/Helm"
  else
    chart="${REPO_ROOT}/contexts/${owner}/ops/Helm"
  fi
  local_values="${chart}/values-local.yaml"
  values="${KIND_DIR}/values/${owner}.yaml"
  if ${DRY_RUN}; then
    args=("${chart}" --namespace "${MVP_NAMESPACE}")
    [[ -f "${local_values}" ]] && args+=(-f "${local_values}")
    [[ -f "${values}" ]] && args+=(-f "${values}")
    helm template "gavel-${owner}" "${args[@]}" >/dev/null
    redacted_action "render Helm migration/release gavel-${owner}"
  else
    args=("${chart}" --namespace "${MVP_NAMESPACE}")
    [[ -f "${local_values}" ]] && args+=(-f "${local_values}")
    [[ -f "${values}" ]] && args+=(-f "${values}")
    args+=(--atomic --wait --wait-for-jobs --timeout 5m)
    helm upgrade --install "gavel-${owner}" "${args[@]}"
  fi
done
