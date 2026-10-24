#!/usr/bin/env bash
set -euo pipefail
KIND_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "${KIND_DIR}/scripts/lib.sh"

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

# Each smoke invocation gets fresh fixtures so retries cannot reuse an expired
# upload URL or an auction that already reached a terminal state.
smoke_run="$(date -u +%s)-$$"
happy_scenario="happy-${smoke_run}"
expiry_scenario="expiry-${smoke_run}"
restart_scenario="restart-${smoke_run}"
for fixture in "${happy_scenario}" "${expiry_scenario}" "${restart_scenario}"; do
  MVP_SEED_SCENARIO="${fixture}" "${KIND_DIR}/scripts/seed.sh"
done

kubectl_mvp port-forward --address 127.0.0.1 service/gavel-bff-gavel-bff 18080:8080 >"${work_dir}/bff.log" 2>&1 &
forward_pids+=("$!")
kubectl_mvp port-forward --address 127.0.0.1 service/gavel-payments-gavel-payments 18083:8080 >"${work_dir}/payments.log" 2>&1 &
forward_pids+=("$!")
kubectl_mvp port-forward --address 127.0.0.1 service/gavel-notify-gavel-notify 18085:8080 >"${work_dir}/notify.log" 2>&1 &
forward_pids+=("$!")
for url in http://127.0.0.1:18080/health/ready http://127.0.0.1:18083/readyz http://127.0.0.1:18085/readyz; do
  for _ in $(seq 1 60); do curl -sf "${url}" >/dev/null && break; sleep 1; done
  curl -sf "${url}" >/dev/null
done

graphql() {
  local url="$1" token="$2" correlation="$3" query="$4" variables="$5"
  local response
  response="$(curl --silent --show-error --fail-with-body \
    -H 'content-type: application/json' -H "authorization: Bearer ${token}" \
    -H "x-correlation-id: ${correlation}" \
    --data "$(jq -cn --arg query "${query}" --argjson variables "${variables}" '{query:$query,variables:$variables}')" \
    "${url}/graphql")"
  jq -e '.errors == null' >/dev/null <<<"${response}" || { jq -c '{errors}' <<<"${response}" >&2; return 1; }
  printf '%s\n' "${response}"
}

token_for() {
  curl --silent --show-error --fail-with-body \
    -H 'host: 127.0.0.1:8080' \
    -X POST -d "grant_type=client_credentials&client_id=$1&client_secret=local-development-only" \
    http://127.0.0.1:18080/default/token | jq -er '.access_token'
}

format_epoch() {
  if date -u -r "$1" '+%Y-%m-%dT%H:%M:%SZ' >/dev/null 2>&1; then
    date -u -r "$1" '+%Y-%m-%dT%H:%M:%SZ'
  else
    date -u -d "@$1" '+%Y-%m-%dT%H:%M:%SZ'
  fi
}

control_token="$(kubectl_mvp get secret gavel-payments-runtime -o jsonpath='{.data.sandbox-control-token}' | base64 --decode)"
notify_operator_token="$(kubectl_mvp get secret gavel-notify-database -o jsonpath='{.data.local-operator-token}' | base64 --decode)"
bid_query='mutation SmokeBid($input: PlaceBidInput!, $key: String!) { placeBid(input: $input, idempotencyKey: $key) { acceptedVersion auction { id status } } }'
reschedule_query='mutation SmokeClose($input: RescheduleAuctionInput!, $key: String!) { rescheduleAuction(input: $input, idempotencyKey: $key) { id status endsAt } }'
auction_query='query SmokeAuction($id: ID!) { auction(id: $id) { id status currentPrice { amount version } } }'

run_scenario() {
  local scenario="$1" round_one_outcome="$2" restart_processes="$3"
  local state="${REPO_ROOT}/.gavel-mvp/seed-${scenario}.json"
  local auction seller_token bidder_one_token bidder_two_token
  auction="$(jq -er '.auctionId' "${state}")"
  seller_token="$(token_for gavel-local-seller)"
  bidder_one_token="$(token_for gavel-local-bidder-one)"
  bidder_two_token="$(token_for gavel-local-bidder-two)"

  graphql http://127.0.0.1:18080 "${bidder_one_token}" "smoke-${scenario}-bid-1" "${bid_query}" \
    "$(jq -cn --arg id "${auction}" --arg key "smoke-${scenario}-bid-1" '{input:{auctionId:$id,maximumAmount:"15000",currency:"USD"},key:$key}')" >/dev/null
  graphql http://127.0.0.1:18080 "${bidder_two_token}" "smoke-${scenario}-bid-2" "${bid_query}" \
    "$(jq -cn --arg id "${auction}" --arg key "smoke-${scenario}-bid-2" '{input:{auctionId:$id,maximumAmount:"20000",currency:"USD"},key:$key}')" >/dev/null

  curl --silent --show-error --fail-with-body -X PUT \
    -H "authorization: Bearer ${control_token}" -H 'content-type: application/json' \
    --data "$(jq -cn --arg outcome "${round_one_outcome}" '{outcome:$outcome}')" \
    "http://127.0.0.1:18083/local/sandbox/outcomes/${auction}/1" >/dev/null
  if [[ "${round_one_outcome}" == "expire" ]]; then
    curl --silent --show-error --fail-with-body -X PUT \
      -H "authorization: Bearer ${control_token}" -H 'content-type: application/json' \
      --data '{"outcome":"success"}' \
      "http://127.0.0.1:18083/local/sandbox/outcomes/${auction}/2" >/dev/null
  fi

  close_at="$(format_epoch "$(( $(date -u +%s) + 8 ))")"
  graphql http://127.0.0.1:18080 "${seller_token}" "smoke-${scenario}-close" "${reschedule_query}" \
    "$(jq -cn --arg id "${auction}" --arg end "${close_at}" --arg key "smoke-${scenario}-close" '{input:{auctionId:$id,scheduledEnd:$end},key:$key}')" >/dev/null

  if [[ "${restart_processes}" == "true" ]]; then
    local accepted_attempt=''
    for _ in $(seq 1 120); do
      accepted_attempt="$(curl --silent --show-error \
        -H "authorization: Bearer ${control_token}" \
        "http://127.0.0.1:18083/local/sandbox/attempts/${auction}/1" || true)"
      jq -e '.status == "recovery_required"' >/dev/null 2>&1 <<<"${accepted_attempt}" && break
      sleep 1
    done
    jq -e '.status == "recovery_required"' >/dev/null <<<"${accepted_attempt}"
    kubectl_mvp rollout restart deployment/gavel-processes-gavel-processes >/dev/null
    kubectl_mvp rollout status deployment/gavel-processes-gavel-processes --timeout=180s >/dev/null
  fi

  if [[ "${round_one_outcome}" == "expire" ]]; then
    for _ in $(seq 1 120); do
      code="$(curl --silent --output /dev/null --write-out '%{http_code}' -X POST \
        -H "authorization: Bearer ${control_token}" \
        "http://127.0.0.1:18083/local/sandbox/attempts/${auction}/1/expire")"
      [[ "${code}" == "200" || "${code}" == "409" ]] && break
      sleep 1
    done
    [[ "${code}" == "200" || "${code}" == "409" ]] || { printf 'round one did not become expirable\n' >&2; return 1; }
  fi

  local response=''
  for _ in $(seq 1 180); do
    response="$(graphql http://127.0.0.1:18080 "${seller_token}" "smoke-${scenario}-read" "${auction_query}" "$(jq -cn --arg id "${auction}" '{id:$id}')")"
    jq -e '.data.auction.status == "SOLD"' >/dev/null <<<"${response}" && break
    sleep 1
  done
  jq -e '.data.auction.status == "SOLD"' >/dev/null <<<"${response}"

  local round_one round_two winner_round winner_notification notification
  round_one="$(curl --silent --show-error --fail-with-body \
    -H "authorization: Bearer ${control_token}" \
    "http://127.0.0.1:18083/local/sandbox/attempts/${auction}/1")"
  if [[ "${round_one_outcome}" == "expire" ]]; then
    round_two="$(curl --silent --show-error --fail-with-body \
      -H "authorization: Bearer ${control_token}" \
      "http://127.0.0.1:18083/local/sandbox/attempts/${auction}/2")"
    jq -e '.status == "expired"' >/dev/null <<<"${round_one}"
    jq -e '.status == "succeeded"' >/dev/null <<<"${round_two}"
    jq -s -e '[.[].status] | map(select(. == "succeeded")) | length == 1' \
      <(printf '%s\n' "${round_one}") <(printf '%s\n' "${round_two}") >/dev/null
    winner_round=2
  else
    jq -e '.status == "succeeded"' >/dev/null <<<"${round_one}"
    winner_round=1
  fi

  winner_notification="settle-auction/${auction}:notify:winner:${winner_round}"
  notification=''
  for _ in $(seq 1 60); do
    notification="$(curl --silent --show-error --get \
      -H "authorization: Bearer ${notify_operator_token}" \
      --data-urlencode "notification_id=${winner_notification}" \
      http://127.0.0.1:18085/local/operator/notification || true)"
    jq -e '.status == "SENT"' >/dev/null 2>&1 <<<"${notification}" && break
    sleep 1
  done
  jq -e --arg id "${winner_notification}" \
    '.notification_id == $id and .status == "SENT" and .attempts == 1' \
    >/dev/null <<<"${notification}"
  printf 'smoke scenario passed: %s (%s)\n' "${scenario}" "${auction}"
}

run_scenario "${happy_scenario}" success false
run_scenario "${expiry_scenario}" expire false
run_scenario "${restart_scenario}" expire true

printf 'all MVP smoke scenarios passed\n'
