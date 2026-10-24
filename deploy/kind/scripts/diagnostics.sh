#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"

require_command kubectl
require_command jq
assert_mvp_scope

stamp="$(date -u +%Y%m%dT%H%M%SZ)"
bundle="${REPO_ROOT}/.gavel-mvp/diagnostics/${stamp}"
mkdir -p "${bundle}/logs"

sanitize() {
  sed -E \
    -e 's/(authorization:?[[:space:]]*(Bearer|Basic))[[:space:]]+[^[:space:]"]+/\1 [REDACTED]/Ig' \
    -e 's/(password|secret|credential|api[-_]?key|token)(["=:][[:space:]]*)[^,[:space:]"}]+/\1\2[REDACTED]/Ig' \
    -e 's#(recovery[_-]?url["=:][[:space:]]*)https?://[^,[:space:]"}]+#\1[REDACTED]#Ig' \
    -e 's/(maximum[_-]?amount["=:][[:space:]]*)[0-9]+/\1[REDACTED]/Ig'
}

kubectl_mvp get all -o wide | sanitize >"${bundle}/workloads.txt" || true
kubectl_mvp get events --sort-by=.lastTimestamp | sanitize >"${bundle}/events.txt" || true
kubectl_mvp get endpointslice -o wide | sanitize >"${bundle}/endpoints.txt" || true
kubectl_mvp exec deployment/kafka-connect -- \
  curl --silent http://localhost:8083/connectors 2>/dev/null \
  | jq -c . | sanitize >"${bundle}/connectors.json" || true

while IFS= read -r pod; do
  safe_name="${pod//\//_}"
  kubectl_mvp logs "${pod}" --all-containers --tail=200 2>&1 \
    | sanitize >"${bundle}/logs/${safe_name}.log" || true
done < <(kubectl_mvp get pods -o name)

printf 'sanitized diagnostics: %s\n' "${bundle}"
