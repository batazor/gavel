#!/usr/bin/env bash
set -euo pipefail

base_url="${GAVEL_LOCAL_BASE_URL:-http://127.0.0.1:8080}"

token() {
  curl --fail --silent --show-error \
    -X POST "${base_url}/$1/token" \
    -H 'content-type: application/x-www-form-urlencoded' \
    --data-urlencode 'grant_type=client_credentials' \
    --data-urlencode "client_id=$2" \
    --data-urlencode 'client_secret=local-development-only' |
    jq -er '.access_token'
}

expect_unauthorized() {
  local label="$1"
  shift
  status="$(curl --silent --output /dev/null --write-out '%{http_code}' "$@")"
  [[ "${status}" == "401" ]] || {
    printf '%s: expected 401, got %s\n' "${label}" "${status}" >&2
    exit 1
  }
}

valid_token="$(token default gavel-local-seller)"
status="$(curl --silent --output /dev/null --write-out '%{http_code}' \
  -H "authorization: Bearer ${valid_token}" \
  -H 'content-type: application/json' \
  --data '{"query":"{__typename}"}' "${base_url}/graphql")"
[[ "${status}" == "200" ]] || { printf 'valid token: expected 200, got %s\n' "${status}" >&2; exit 1; }

expect_unauthorized missing-token -H 'content-type: application/json' --data '{"query":"{__typename}"}' "${base_url}/graphql"
expect_unauthorized wrong-audience -H "authorization: Bearer $(token default gavel-local-wrong-audience)" "${base_url}/graphql"
expect_unauthorized wrong-issuer -H "authorization: Bearer $(token foreign gavel-local-foreign-issuer)" "${base_url}/graphql"
expect_unauthorized invalid-signature -H "authorization: Bearer ${valid_token}x" "${base_url}/graphql"

printf 'local JWT issuer/audience/signature checks passed\n'
