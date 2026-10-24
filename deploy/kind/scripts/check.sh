#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"
require_command kubectl
require_command jq

rendered="$(mktemp)"
trap 'rm -f "${rendered}"' EXIT
kubectl kustomize "${KIND_DIR}" >"${rendered}"
for workload in postgresql temporal redpanda kafka-connect spicedb local-jwt mailpit redis; do
  grep -q "name: ${workload}" "${rendered}" || { printf 'missing workload: %s\n' "${workload}" >&2; exit 1; }
done
if grep -Eqi '(password|secret|credential):[[:space:]]+[^{$@]' "${KIND_DIR}"/platform/*.yaml "${KIND_DIR}"/auth/*.yaml; then
  printf 'possible committed credential in platform manifests\n' >&2
  exit 1
fi
for script in "${KIND_DIR}"/scripts/*.sh; do bash -n "${script}"; done
"${KIND_DIR}/scripts/platform.sh" --dry-run >/dev/null
for template in "${KIND_DIR}"/connectors/*.json.tpl; do
  sed 's/@@CDC_PASSWORD@@/redacted/g' "${template}" | jq empty
done
"${KIND_DIR}/scripts/bootstrap.sh" --dry-run >/dev/null
"${KIND_DIR}/tests/authz-bootstrap-contract.sh"
"${KIND_DIR}/tests/command-inventory.sh"
"${KIND_DIR}/tests/seed-contract.sh"
"${KIND_DIR}/tests/readiness-contract.sh"
"${KIND_DIR}/tests/smoke-contract.sh"
"${KIND_DIR}/tests/docs-contract.sh"
printf 'deploy/kind static inventory and dry-run checks passed\n'
