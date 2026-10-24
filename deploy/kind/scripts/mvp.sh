#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"

usage() {
  cat <<'EOF'
usage: deploy/kind/scripts/mvp.sh <platform|up|down|seed|ready|smoke|e2e|diagnostics|check>

The orchestrator is intentionally pinned to cluster/context/namespace gavel-mvp.
Domain seed, readiness, smoke and diagnostic behavior belongs to their dedicated
scripts and is dispatched here without being reimplemented.
EOF
}

phase_path() {
  case "$1" in
    seed) printf '%s/scripts/seed.sh\n' "${KIND_DIR}" ;;
    ready) printf '%s/scripts/ready.sh\n' "${KIND_DIR}" ;;
    smoke) printf '%s/tests/mvp-smoke.sh\n' "${KIND_DIR}" ;;
    e2e) printf '%s/tests/mvp-e2e.sh\n' "${KIND_DIR}" ;;
    diagnostics) printf '%s/scripts/diagnostics.sh\n' "${KIND_DIR}" ;;
    *) printf 'unknown MVP phase: %s\n' "$1" >&2; return 2 ;;
  esac
}

require_phase() {
  local phase="$1"
  local handler
  handler="$(phase_path "${phase}")"
  [[ -x "${handler}" ]] || {
    printf 'MVP %s handler is not installed yet: %s\n' "${phase}" "${handler}" >&2
    printf 'Complete the corresponding OpenSpec task before running this command.\n' >&2
    return 3
  }
  "${handler}"
}

cluster_exists() {
  kind get clusters 2>/dev/null | grep -Fx "${EXPECTED_MVP_CLUSTER}" >/dev/null
}

create_or_reuse_cluster() {
  require_command kind
  require_command kubectl
  require_command helm
  require_command docker
  if cluster_exists; then
    printf 'reuse kind cluster %s\n' "${EXPECTED_MVP_CLUSTER}"
  else
    kind create cluster --name "${EXPECTED_MVP_CLUSTER}" --config "${KIND_DIR}/cluster.yaml"
  fi
  kind export kubeconfig --name "${EXPECTED_MVP_CLUSTER}" >/dev/null
  kubectl --context "${EXPECTED_MVP_CONTEXT}" cluster-info >/dev/null
  # Production charts spread replicas by zone. kind nodes do not advertise a
  # zone by default, so even a single local replica would otherwise remain
  # unschedulable.
  kubectl --context "${EXPECTED_MVP_CONTEXT}" label nodes --all \
    topology.kubernetes.io/zone=kind-local --overwrite >/dev/null
}

run_up() {
  ready_handler="$(phase_path ready)"
  [[ -x "${ready_handler}" ]] || {
    printf 'MVP ready handler is not installed yet: %s\n' "${ready_handler}" >&2
    printf 'Refusing to create a cluster until OpenSpec task 6.3 supplies the readiness gate.\n' >&2
    return 3
  }
  run_platform
  if ! "${KIND_DIR}/scripts/bootstrap.sh"; then
    printf 'MVP bootstrap failed; attempting bounded diagnostics\n' >&2
    diagnostics_handler="$(phase_path diagnostics)"
    [[ -x "${diagnostics_handler}" ]] && "${diagnostics_handler}" || true
    return 1
  fi
  require_phase ready
}

run_platform() {
  create_or_reuse_cluster
  "${KIND_DIR}/scripts/platform.sh"
}

run_down() {
  require_command kind
  if cluster_exists; then
    kind delete cluster --name "${EXPECTED_MVP_CLUSTER}"
  else
    printf 'kind cluster %s is already absent\n' "${EXPECTED_MVP_CLUSTER}"
  fi
}

check_inventory() {
  assert_mvp_scope
  [[ -f "${KIND_DIR}/cluster.yaml" ]]
  [[ -f "${KIND_DIR}/inventory.yaml" ]]
  [[ -x "${KIND_DIR}/scripts/platform.sh" ]]
  [[ -x "${KIND_DIR}/scripts/bootstrap.sh" ]]
  for phase in seed ready smoke e2e diagnostics; do
    phase_path "${phase}" >/dev/null
  done
  printf 'MVP dispatcher targets cluster=%s context=%s namespace=%s\n' \
    "${EXPECTED_MVP_CLUSTER}" "${EXPECTED_MVP_CONTEXT}" "${EXPECTED_MVP_NAMESPACE}"
}

assert_mvp_scope
case "${1:-}" in
  platform) run_platform ;;
  up) run_up ;;
  down) run_down ;;
  seed) require_phase seed ;;
  ready) require_phase ready ;;
  smoke) require_phase ready; require_phase smoke ;;
  e2e) require_phase ready; require_phase e2e ;;
  diagnostics) require_phase diagnostics ;;
  check|--check) check_inventory ;;
  -h|--help|help) usage ;;
  *) usage >&2; exit 2 ;;
esac
