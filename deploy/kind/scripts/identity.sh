#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"

DRY_RUN=false
[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=true

if ${DRY_RUN}; then
  redacted_action "sync local JWT opaque principal claims from the Authz owner bootstrap report"
  exit 0
fi

require_command kubectl
require_command jq

job="$(kubectl_mvp get jobs \
  -l app.kubernetes.io/name=gavel-authz-bootstrap,app.kubernetes.io/instance=gavel-authz \
  --sort-by=.metadata.creationTimestamp \
  -o jsonpath='{.items[-1:].metadata.name}')"
[[ -n "${job}" ]] || { printf 'Authz bootstrap Job was not found\n' >&2; exit 1; }
kubectl_mvp wait --for=condition=complete "job/${job}" --timeout=180s >/dev/null

report="$(kubectl_mvp logs "job/${job}" -c bootstrap \
  | jq -Rrc 'fromjson? | select(.actors != null)' \
  | tail -n 1)"
for actor in seller bidder-one bidder-two; do
  principal="$(jq -er --arg actor "${actor}" '.actors[$actor] | select(startswith("u1_"))' <<<"${report}")"
  [[ -n "${principal}" ]] || { printf 'missing opaque principal for %s\n' "${actor}" >&2; exit 1; }
done

config="$(kubectl_mvp get configmap local-jwt-config -o jsonpath='{.data.json-config}')"
updated="$(jq -c \
  --arg seller "$(jq -r '.actors.seller' <<<"${report}")" \
  --arg bidder_one "$(jq -r '.actors["bidder-one"]' <<<"${report}")" \
  --arg bidder_two "$(jq -r '.actors["bidder-two"]' <<<"${report}")" \
  '(.tokenCallbacks[].requestMappings[] | select(.match == "gavel-local-seller") | .claims.principal_id) = $seller
   | (.tokenCallbacks[].requestMappings[] | select(.match == "gavel-local-bidder-one") | .claims.principal_id) = $bidder_one
   | (.tokenCallbacks[].requestMappings[] | select(.match == "gavel-local-bidder-two") | .claims.principal_id) = $bidder_two' \
  <<<"${config}")"

patch="$(jq -cn --arg config "${updated}" '{data:{"json-config":$config}}')"
kubectl_mvp patch configmap local-jwt-config --type=merge -p "${patch}" >/dev/null
kubectl_mvp rollout restart deployment/local-jwt >/dev/null
kubectl_mvp rollout status deployment/local-jwt --timeout=180s >/dev/null
printf 'local JWT identities synchronized from Authz bootstrap\n'
