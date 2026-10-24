#!/usr/bin/env bash
set -euo pipefail

KIND_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${KIND_DIR}/../.." && pwd)"
MVP_NAMESPACE="${MVP_NAMESPACE:-gavel-mvp}"
MVP_CLUSTER="${MVP_CLUSTER:-gavel-mvp}"
MVP_CONTEXT="${MVP_CONTEXT:-kind-${MVP_CLUSTER}}"
EXPECTED_MVP_NAMESPACE="gavel-mvp"
EXPECTED_MVP_CLUSTER="gavel-mvp"
EXPECTED_MVP_CONTEXT="kind-gavel-mvp"

assert_mvp_scope() {
  [[ "${MVP_CLUSTER}" == "${EXPECTED_MVP_CLUSTER}" ]] || {
    printf 'refusing non-MVP cluster target: %s (expected %s)\n' "${MVP_CLUSTER}" "${EXPECTED_MVP_CLUSTER}" >&2
    exit 2
  }
  [[ "${MVP_NAMESPACE}" == "${EXPECTED_MVP_NAMESPACE}" ]] || {
    printf 'refusing non-MVP namespace target: %s (expected %s)\n' "${MVP_NAMESPACE}" "${EXPECTED_MVP_NAMESPACE}" >&2
    exit 2
  }
  [[ "${MVP_CONTEXT}" == "${EXPECTED_MVP_CONTEXT}" ]] || {
    printf 'refusing non-MVP context target: %s (expected %s)\n' "${MVP_CONTEXT}" "${EXPECTED_MVP_CONTEXT}" >&2
    exit 2
  }
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || {
    printf 'required command is missing: %s\n' "$1" >&2
    exit 1
  }
}

kubectl_mvp() {
  assert_mvp_scope
  kubectl --context "${MVP_CONTEXT}" --namespace "${MVP_NAMESPACE}" "$@"
}

random_hex() {
  openssl rand -hex "$1"
}

random_base64_32() {
  openssl rand -base64 32 | tr -d '\n'
}

redacted_action() {
  printf '[dry-run] %s\n' "$*"
}
