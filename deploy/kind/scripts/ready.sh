#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"

require_command kubectl
require_command curl
require_command jq
assert_mvp_scope

work_dir="$(mktemp -d)"
forward_pids=()
cleanup() {
  for pid in "${forward_pids[@]-}"; do [[ -n "${pid}" ]] && kill "${pid}" >/dev/null 2>&1 || true; done
  for pid in "${forward_pids[@]-}"; do [[ -n "${pid}" ]] && wait "${pid}" >/dev/null 2>&1 || true; done
  rm -rf "${work_dir}"
}
trap cleanup EXIT

for workload in \
  statefulset/postgresql deployment/temporal statefulset/redpanda \
  deployment/kafka-connect deployment/spicedb deployment/local-jwt deployment/mailpit \
  deployment/redis \
  deployment/gavel-authz-gavel-authz deployment/gavel-catalog-gavel-catalog \
  deployment/gavel-auction-gavel-auction deployment/gavel-payments-gavel-payments \
  deployment/gavel-notify-gavel-notify deployment/gavel-processes-gavel-processes \
  deployment/gavel-bff-gavel-bff deployment/gavel-web-gavel-web; do
  kubectl_mvp rollout status "${workload}" --timeout=180s >/dev/null
done

for service in \
  gavel-authz-gavel-authz gavel-catalog-gavel-catalog gavel-auction-gavel-auction \
  gavel-payments-gavel-payments gavel-notify-gavel-notify; do
  ready_addresses="$(kubectl_mvp get endpointslice \
    -l "kubernetes.io/service-name=${service}" \
    -o json | jq '[.items[].endpoints[] | select(.conditions.ready == true)] | length')"
  [[ "${ready_addresses}" -gt 0 ]] || { printf 'owner endpoint is not ready: %s\n' "${service}" >&2; exit 1; }
done

kubectl_mvp port-forward --address 127.0.0.1 service/gavel-bff-gavel-bff 18080:8080 >"${work_dir}/bff.log" 2>&1 &
forward_pids+=("$!")
kubectl_mvp port-forward --address 127.0.0.1 service/gavel-processes-gavel-processes 18084:8080 >"${work_dir}/processes.log" 2>&1 &
forward_pids+=("$!")
kubectl_mvp port-forward --address 127.0.0.1 service/mailpit 18025:8025 >"${work_dir}/mailpit.log" 2>&1 &
forward_pids+=("$!")

wait_get() {
  local url="$1"
  for _ in $(seq 1 60); do
    curl --silent --fail --max-time 2 "${url}" >/dev/null 2>&1 && return 0
    sleep 1
  done
  printf 'readiness HTTP probe failed: %s\n' "${url}" >&2
  return 1
}
wait_get http://127.0.0.1:18080/health/ready
wait_get http://127.0.0.1:18084/readyz
wait_get http://127.0.0.1:18025/api/v1/info

for connector in gavel-catalog-v1 gavel-auction-public-v1 gavel-auction-restricted-v1 gavel-payments-v1; do
  status="$(kubectl_mvp exec deployment/kafka-connect -- \
    curl --silent --fail "http://localhost:8083/connectors/${connector}/status")"
  jq -e '.connector.state == "RUNNING" and ([.tasks[].state] | all(. == "RUNNING"))' >/dev/null <<<"${status}" || {
    printf 'connector is not running: %s\n' "${connector}" >&2
    exit 1
  }
done

graphql_probe='{"query":"query Readiness { lots(first: 1) { edges { node { id } } pageInfo { hasNextPage } } }"}'
token="$(curl --silent --show-error --fail-with-body \
  -H 'host: 127.0.0.1:8080' \
  -X POST -d 'grant_type=client_credentials&client_id=gavel-local-seller&client_secret=local-development-only' \
  http://127.0.0.1:18080/default/token | jq -er '.access_token')"
response="$(curl --silent --show-error --fail-with-body \
  -H 'content-type: application/json' -H "authorization: Bearer ${token}" \
  --data "${graphql_probe}" http://127.0.0.1:18080/graphql)"
jq -e '.errors == null and .data.lots.pageInfo.hasNextPage != null' >/dev/null <<<"${response}"

printf 'MVP ready: graph, owner endpoints, broker routes, Temporal workers and Mailpit are healthy\n'
