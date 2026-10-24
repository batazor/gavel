#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"

require_command kubectl
require_command curl
require_command jq
require_command openssl
assert_mvp_scope

state_dir="${REPO_ROOT}/.gavel-mvp"
mkdir -p "${state_dir}"
scenario="${MVP_SEED_SCENARIO:-default}"
[[ "${scenario}" =~ ^[a-z0-9-]{1,32}$ ]] || { printf 'invalid MVP_SEED_SCENARIO\n' >&2; exit 2; }
state_file="${state_dir}/seed-${scenario}.json"
cluster_uid="$(kubectl --context "${MVP_CONTEXT}" get namespace "${MVP_NAMESPACE}" -o jsonpath='{.metadata.uid}')"
if [[ -f "${state_file}" ]] && [[ "$(jq -r '.clusterUid // empty' "${state_file}")" == "${cluster_uid}" ]]; then
  seed_key="$(jq -er '.seedKey' "${state_file}")"
  scheduled_end="$(jq -er '.scheduledEnd' "${state_file}")"
  hard_deadline="$(jq -er '.hardDeadline' "${state_file}")"
else
  seed_key="${scenario}-${cluster_uid:0:12}"
  end_seconds="${MVP_SEED_END_SECONDS:-300}"
  hard_deadline_seconds="${MVP_SEED_HARD_DEADLINE_SECONDS:-300}"
  [[ "${end_seconds}" =~ ^[0-9]+$ && "${end_seconds}" -ge 15 && "${end_seconds}" -le 3600 ]] || {
    printf 'MVP_SEED_END_SECONDS must be between 15 and 3600\n' >&2
    exit 2
  }
  [[ "${hard_deadline_seconds}" =~ ^[0-9]+$ && "${hard_deadline_seconds}" -ge 30 && "${hard_deadline_seconds}" -le 7200 ]] || {
    printf 'MVP_SEED_HARD_DEADLINE_SECONDS must be between 30 and 7200\n' >&2
    exit 2
  }
  end_epoch="$(( $(date -u +%s) + end_seconds ))"
  hard_epoch="$(( end_epoch + hard_deadline_seconds ))"
  if date -u -r "${end_epoch}" '+%Y-%m-%dT%H:%M:%SZ' >/dev/null 2>&1; then
    scheduled_end="$(date -u -r "${end_epoch}" '+%Y-%m-%dT%H:%M:%SZ')"
    hard_deadline="$(date -u -r "${hard_epoch}" '+%Y-%m-%dT%H:%M:%SZ')"
  else
    scheduled_end="$(date -u -d "@${end_epoch}" '+%Y-%m-%dT%H:%M:%SZ')"
    hard_deadline="$(date -u -d "@${hard_epoch}" '+%Y-%m-%dT%H:%M:%SZ')"
  fi
  jq -n --arg clusterUid "${cluster_uid}" --arg seedKey "${seed_key}" \
    --arg scheduledEnd "${scheduled_end}" --arg hardDeadline "${hard_deadline}" \
    '{clusterUid:$clusterUid,seedKey:$seedKey,scheduledEnd:$scheduledEnd,hardDeadline:$hardDeadline}' \
    >"${state_file}"
fi
work_dir="$(mktemp -d)"
forward_pids=()
cleanup() {
  for pid in "${forward_pids[@]-}"; do [[ -n "${pid}" ]] && kill "${pid}" >/dev/null 2>&1 || true; done
  for pid in "${forward_pids[@]-}"; do [[ -n "${pid}" ]] && wait "${pid}" >/dev/null 2>&1 || true; done
  rm -rf "${work_dir}"
}
trap cleanup EXIT

start_forward() {
  local name="$1" resource="$2" local_port="$3" remote_port="$4"
  kubectl_mvp port-forward --address 127.0.0.1 "${resource}" "${local_port}:${remote_port}" \
    >"${work_dir}/${name}.log" 2>&1 &
  forward_pids+=("$!")
}

wait_http() {
  local url="$1"
  for _ in $(seq 1 60); do
    curl --silent --fail --max-time 2 "${url}" >/dev/null 2>&1 && return 0
    sleep 1
  done
  printf 'timed out waiting for %s\n' "${url}" >&2
  return 1
}

graphql() {
  local url="$1" token="$2" correlation="$3" query="$4" variables="$5"
  local response
  response="$(curl --silent --show-error --fail-with-body \
    -H 'content-type: application/json' \
    -H "authorization: Bearer ${token}" \
    -H "x-correlation-id: ${correlation}" \
    --data "$(jq -cn --arg query "${query}" --argjson variables "${variables}" '{query:$query,variables:$variables}')" \
    "${url}/graphql")"
  if ! jq -e '.errors == null' >/dev/null <<<"${response}"; then
    jq -c '{errors}' <<<"${response}" >&2
    return 1
  fi
  printf '%s\n' "${response}"
}

jwt_config="$(kubectl_mvp get configmap local-jwt-config -o jsonpath='{.data.json-config}')"
principal_for() {
  jq -er --arg client "$1" \
    '.tokenCallbacks[].requestMappings[] | select(.match == $client) | .claims.principal_id | select(startswith("u1_"))' \
    <<<"${jwt_config}"
}

seller="$(principal_for gavel-local-seller)"
bidder_one="$(principal_for gavel-local-bidder-one)"
bidder_two="$(principal_for gavel-local-bidder-two)"

start_forward bff service/gavel-bff-gavel-bff 18080 8080
start_forward catalog service/gavel-catalog-gavel-catalog 18081 8080
start_forward payments service/gavel-payments-gavel-payments 18083 8080
wait_http http://127.0.0.1:18080/health/ready
wait_http http://127.0.0.1:18081/readyz
wait_http http://127.0.0.1:18083/readyz

token_for() {
  curl --silent --show-error --fail-with-body \
    -H 'host: 127.0.0.1:8080' \
    -X POST -d "grant_type=client_credentials&client_id=$1&client_secret=local-development-only" \
    http://127.0.0.1:18080/default/token | jq -er '.access_token'
}
seller_token="$(token_for gavel-local-seller)"
bidder_one_token="$(token_for gavel-local-bidder-one)"
bidder_two_token="$(token_for gavel-local-bidder-two)"

image_file="${work_dir}/seed-image.png"
printf 'gavel-mvp-seed-image-v1' >"${image_file}"
image_length="$(wc -c <"${image_file}" | tr -d ' ')"
image_checksum="$(openssl dgst -sha256 -binary "${image_file}" | openssl base64 -A)"

upload_query='mutation SeedUpload($input: RequestLotImageUploadInput!, $key: String!) { requestLotImageUpload(input: $input, idempotencyKey: $key) { imageId uploadUrl } }'
upload_vars="$(jq -cn --arg checksum "${image_checksum}" --argjson length "${image_length}" \
  --arg key "mvp-seed-image-${seed_key}" \
  '{input:{contentType:"image/png",contentLength:$length,checksumSha256:$checksum},key:$key}')"
upload="$(graphql http://127.0.0.1:18080 "${seller_token}" mvp-seed-upload "${upload_query}" "${upload_vars}")"
image_id="$(jq -er '.data.requestLotImageUpload.imageId' <<<"${upload}")"
upload_url="$(jq -er '.data.requestLotImageUpload.uploadUrl' <<<"${upload}")"

complete_query='mutation CompleteSeedUpload($id: ID!, $key: String!) { completeLotImageUpload(imageId: $id, idempotencyKey: $key) { imageId status } }'
complete_vars="$(jq -cn --arg id "${image_id}" --arg key "mvp-seed-image-complete-${seed_key}" '{id:$id,key:$key}')"
curl --silent --show-error --fail-with-body \
  --connect-to gavel-catalog-gavel-catalog:8080:127.0.0.1:18081 \
  -X PUT -H 'content-type: image/png' \
  -H "content-length: ${image_length}" \
  -H "x-amz-checksum-sha256: ${image_checksum}" \
  --data-binary "@${image_file}" "${upload_url}" >/dev/null
complete="$(graphql http://127.0.0.1:18080 "${seller_token}" mvp-seed-complete "${complete_query}" "${complete_vars}")"
jq -e '.data.completeLotImageUpload.status == "READY" or .data.completeLotImageUpload.status == "ATTACHED"' >/dev/null <<<"${complete}"

list_query='mutation SeedLot($input: ListLotInput!, $key: String!) { listLot(input: $input, idempotencyKey: $key) { id title images { id } } }'
list_vars="$(jq -cn --arg image "${image_id}" --arg scenario "${scenario}" \
  --arg end "${scheduled_end}" --arg hard "${hard_deadline}" --arg key "mvp-seed-lot-${seed_key}" \
  '{input:{title:("Gavel MVP " + $scenario + " lot"),description:"Idempotent local vertical-slice fixture",images:[{imageId:$image,alt:"Gavel seed fixture"}],startPrice:"10000",currency:"USD",bidStep:"500",scheduledEnd:$end,inactivitySeconds:30,hardDeadline:$hard},key:$key}')"
lot="$(graphql http://127.0.0.1:18080 "${seller_token}" mvp-seed-lot "${list_query}" "${list_vars}")"
lot_id="$(jq -er '.data.listLot.id' <<<"${lot}")"

registration_query='mutation SeedInstrument($input: BeginRegistrationInput!, $key: String!) { beginPaymentInstrumentRegistration(input: $input, idempotencyKey: $key) { instrument { id status } providerUrl } }'
control_token="$(kubectl_mvp get secret gavel-payments-runtime -o jsonpath='{.data.sandbox-control-token}' | base64 --decode)"
seed_instrument() {
  local actor="$1" token="$2"
  local variables registration provider_url session_ref
  variables="$(jq -cn --arg key "mvp-seed-${actor}-instrument-v1" --arg email "${actor}@gavel.local" \
    '{input:{recoveryEmail:$email,termsVersion:"mvp-v1"},key:$key}')"
  registration="$(graphql http://127.0.0.1:18080 "${token}" "mvp-seed-${actor}-instrument" "${registration_query}" "${variables}")"
  provider_url="$(jq -er '.data.beginPaymentInstrumentRegistration.providerUrl' <<<"${registration}")"
  session_ref="${provider_url##*/instruments/}"
  session_ref="${session_ref%%/ready*}"
  curl --silent --show-error --fail-with-body -X POST \
    -H "authorization: Bearer ${control_token}" \
    "http://127.0.0.1:18083/local/sandbox/instruments/${session_ref}/ready" >/dev/null
}
seed_instrument bidder-one "${bidder_one_token}"
seed_instrument bidder-two "${bidder_two_token}"

auction_query='query SeedAuction($id: ID!) { auction(id: $id) { id lotId status currentPrice { amount currency version } viewer { canBid } } }'
auction_vars="$(jq -cn --arg id "${lot_id}" '{id:$id}')"
auction=''
for _ in $(seq 1 60); do
  auction="$(graphql http://127.0.0.1:18080 "${seller_token}" mvp-seed-auction "${auction_query}" "${auction_vars}")"
  jq -e '.data.auction.status == "OPEN"' >/dev/null <<<"${auction}" && break
  sleep 1
done
jq -e '.data.auction.status == "OPEN"' >/dev/null <<<"${auction}" || {
  printf 'seed auction was not opened by the Catalog event route\n' >&2
  exit 1
}

for bidder_token in "${bidder_one_token}" "${bidder_two_token}"; do
  bidder_view=''
  for _ in $(seq 1 60); do
    bidder_view="$(graphql http://127.0.0.1:18080 "${bidder_token}" mvp-seed-bidder-ready "${auction_query}" "${auction_vars}")"
    jq -e '.data.auction.status == "OPEN" and .data.auction.viewer.canBid == true' >/dev/null <<<"${bidder_view}" && break
    sleep 1
  done
  jq -e '.data.auction.status == "OPEN" and .data.auction.viewer.canBid == true' >/dev/null <<<"${bidder_view}" || {
    printf 'seed bidder did not become eligible before the auction deadline\n' >&2
    exit 1
  }
done

jq -n \
  --arg clusterUid "${cluster_uid}" \
  --arg seedKey "${seed_key}" \
  --arg scheduledEnd "${scheduled_end}" \
  --arg hardDeadline "${hard_deadline}" \
  --arg lotId "${lot_id}" \
  --arg auctionId "$(jq -r '.data.auction.id' <<<"${auction}")" \
  --arg seller "${seller}" \
  --arg bidderOne "${bidder_one}" \
  --arg bidderTwo "${bidder_two}" \
  '{clusterUid:$clusterUid,seedKey:$seedKey,scheduledEnd:$scheduledEnd,hardDeadline:$hardDeadline,lotId:$lotId,auctionId:$auctionId,principals:{seller:$seller,bidderOne:$bidderOne,bidderTwo:$bidderTwo}}' \
  >"${state_file}"
printf 'MVP seed ready: lot=%s auction=%s\n' "${lot_id}" "${lot_id}"
