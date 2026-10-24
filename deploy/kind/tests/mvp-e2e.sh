#!/usr/bin/env bash
set -euo pipefail
KIND_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "${KIND_DIR}/scripts/lib.sh"

require_command kubectl
require_command curl
require_command jq
require_command npm
assert_mvp_scope

work_dir="$(mktemp -d)"
forward_pids=()
cleanup() {
  for pid in "${forward_pids[@]-}"; do [[ -n "${pid}" ]] && kill "${pid}" >/dev/null 2>&1 || true; done
  for pid in "${forward_pids[@]-}"; do [[ -n "${pid}" ]] && wait "${pid}" >/dev/null 2>&1 || true; done
  rm -rf "${work_dir}"
}
trap cleanup EXIT

scenario="e2e-$(date -u +%H%M%S)-$$"
MVP_SEED_SCENARIO="${scenario}" \
MVP_SEED_END_SECONDS=45 \
MVP_SEED_HARD_DEADLINE_SECONDS=180 \
  "${KIND_DIR}/scripts/seed.sh"
state_file="${REPO_ROOT}/.gavel-mvp/seed-${scenario}.json"
auction_id="$(jq -er '.auctionId' "${state_file}")"

kubectl_mvp port-forward --address 127.0.0.1 service/gavel-web-gavel-web 4173:8080 \
  >"${work_dir}/web.log" 2>&1 &
forward_pids+=("$!")
kubectl_mvp port-forward --address 127.0.0.1 service/gavel-payments-gavel-payments 18083:8080 \
  >"${work_dir}/payments.log" 2>&1 &
forward_pids+=("$!")

wait_http() {
  local url="$1"
  for _ in $(seq 1 60); do
    curl --silent --fail --max-time 2 "${url}" >/dev/null 2>&1 && return 0
    sleep 1
  done
  printf 'timed out waiting for %s\n' "${url}" >&2
  return 1
}
wait_http http://127.0.0.1:4173/readyz
wait_http http://127.0.0.1:8080/health/ready
wait_http http://127.0.0.1:18083/readyz
wait_http http://127.0.0.1:8025/api/v1/info

control_token="$(kubectl_mvp get secret gavel-payments-runtime -o jsonpath='{.data.sandbox-control-token}' | base64 --decode)"
curl --silent --show-error --fail-with-body -X PUT \
  -H "authorization: Bearer ${control_token}" \
  -H 'content-type: application/json' \
  --data '{"outcome":"success"}' \
  "http://127.0.0.1:18083/local/sandbox/outcomes/${auction_id}/1" >/dev/null

GAVEL_E2E_BASE_URL=http://127.0.0.1:4173 \
GAVEL_E2E_MAILPIT_URL=http://127.0.0.1:8025 \
GAVEL_E2E_STATE_FILE="${state_file}" \
  npm run e2e --workspace=@gavel/web
