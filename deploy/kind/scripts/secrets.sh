#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"

DRY_RUN=false
[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=true

if ${DRY_RUN}; then
  for name in $(awk '/^    - gavel-/ {print $2}' "${KIND_DIR}/inventory.yaml"); do
    redacted_action "ensure Secret/${name} (values omitted)"
  done
  exit 0
fi

require_command kubectl
require_command openssl
kubectl --context "${MVP_CONTEXT}" apply -f "${KIND_DIR}/platform/namespace.yaml" >/dev/null

secret_exists() {
  kubectl_mvp get secret "$1" >/dev/null 2>&1
}

secret_value() {
  kubectl_mvp get secret "$1" -o "jsonpath={.data.$2}" | base64 --decode
}

apply_secret() {
  local name="$1"
  shift
  if secret_exists "${name}"; then
    printf 'preserve Secret/%s\n' "${name}"
    return
  fi
  kubectl_mvp create secret generic "${name}" "$@" --dry-run=client -o yaml | kubectl_mvp apply -f - >/dev/null
  printf 'created Secret/%s\n' "${name}"
}

if secret_exists gavel-platform-postgres; then
  pg_admin="$(secret_value gavel-platform-postgres admin-password)"
  catalog_pw="$(secret_value gavel-platform-postgres catalog-password)"
  auction_pw="$(secret_value gavel-platform-postgres auction-password)"
  payments_pw="$(secret_value gavel-platform-postgres payments-password)"
  notify_pw="$(secret_value gavel-platform-postgres notify-password)"
  authz_pw="$(secret_value gavel-platform-postgres authz-password)"
  spicedb_pw="$(secret_value gavel-platform-postgres spicedb-password)"
  cdc_pw="$(secret_value gavel-platform-postgres cdc-password)"
else
  pg_admin="$(random_hex 24)"
  catalog_pw="$(random_hex 24)"
  auction_pw="$(random_hex 24)"
  payments_pw="$(random_hex 24)"
  notify_pw="$(random_hex 24)"
  authz_pw="$(random_hex 24)"
  spicedb_pw="$(random_hex 24)"
  cdc_pw="$(random_hex 24)"
fi

apply_secret gavel-platform-postgres \
  --from-literal=admin-password="${pg_admin}" \
  --from-literal=catalog-password="${catalog_pw}" \
  --from-literal=auction-password="${auction_pw}" \
  --from-literal=payments-password="${payments_pw}" \
  --from-literal=notify-password="${notify_pw}" \
  --from-literal=authz-password="${authz_pw}" \
  --from-literal=spicedb-password="${spicedb_pw}" \
  --from-literal=cdc-password="${cdc_pw}"

redpanda_admin_pw="$(secret_exists gavel-redpanda-admin && secret_value gavel-redpanda-admin password || random_hex 24)"
redpanda_connect_pw="$(secret_exists gavel-redpanda-connect && secret_value gavel-redpanda-connect password || random_hex 24)"
redpanda_processes_pw="$(secret_exists gavel-redpanda-processes && secret_value gavel-redpanda-processes password || random_hex 24)"
redpanda_auction_pw="$(secret_exists gavel-redpanda-auction && secret_value gavel-redpanda-auction password || random_hex 24)"
spicedb_key="$(secret_exists gavel-spicedb && secret_value gavel-spicedb preshared-key || random_hex 24)"

apply_secret gavel-redpanda-admin --from-literal=username=gavel-admin --from-literal=password="${redpanda_admin_pw}"
apply_secret gavel-redpanda-connect --from-literal=username=gavel-connect --from-literal=password="${redpanda_connect_pw}"
apply_secret gavel-redpanda-processes --from-literal=username=gavel-processes --from-literal=password="${redpanda_processes_pw}"
apply_secret gavel-redpanda-auction --from-literal=username=gavel-auction --from-literal=password="${redpanda_auction_pw}"
apply_secret gavel-spicedb \
  --from-literal=datastore-uri="postgres://spicedb:${spicedb_pw}@postgresql:5432/gavel_spicedb?sslmode=disable" \
  --from-literal=preshared-key="${spicedb_key}"

apply_secret gavel-auction-database --from-literal=url="postgres://auction:${auction_pw}@postgresql:5432/gavel_auction?sslmode=disable"
apply_secret gavel-auction-command-fingerprint --from-literal=key-hex="$(random_hex 32)"
apply_secret gavel-catalog-database --from-literal=url="postgres://catalog:${catalog_pw}@postgresql:5432/gavel_catalog?sslmode=disable"
apply_secret gavel-catalog-object-storage --from-literal=endpoint=http://unused.invalid --from-literal=access-key=local-catalog --from-literal=secret-key="$(random_hex 24)"
apply_secret gavel-catalog-hmac --from-literal=fingerprint="$(random_hex 32)" --from-literal=cursor="$(random_hex 32)"
apply_secret gavel-payments-runtime \
  --from-literal=database-url="postgres://payments:${payments_pw}@postgresql:5432/gavel_payments?sslmode=disable" \
  --from-literal=provider-secret="local-sandbox-$(random_hex 16)" \
  --from-literal=callback-secret="$(random_hex 32)" \
  --from-literal=fingerprint-secret="$(random_hex 32)" \
  --from-literal=sandbox-control-token="$(random_hex 32)"
apply_secret gavel-authz-database --from-literal=url="postgres://authz:${authz_pw}@postgresql:5432/gavel_authz?sslmode=disable"
kubectl_mvp create secret generic gavel-authz-spicedb \
  --from-literal=endpoint=http://spicedb:50051 \
  --from-literal=zed-endpoint=spicedb:50051 \
  --from-literal=credential="${spicedb_key}" \
  --dry-run=client -o yaml | kubectl_mvp apply -f - >/dev/null
printf 'ensured Secret/%s\n' gavel-authz-spicedb
apply_secret gavel-authz-hmac --from-literal=principal="$(random_base64_32)" --from-literal=fingerprint="$(random_base64_32)"
notify_database_url="postgres://notify:${notify_pw}@postgresql:5432/gavel_notify?sslmode=disable"
notify_operator_token="$(random_hex 32)"
if secret_exists gavel-notify-database; then
  notify_database_url="$(secret_value gavel-notify-database url)"
  if kubectl_mvp get secret gavel-notify-database -o jsonpath='{.data.local-operator-token}' | grep -q .; then
    notify_operator_token="$(secret_value gavel-notify-database local-operator-token)"
  fi
fi
kubectl_mvp create secret generic gavel-notify-database \
  --from-literal=url="${notify_database_url}" \
  --from-literal=local-operator-token="${notify_operator_token}" \
  --dry-run=client -o yaml | kubectl_mvp apply -f - >/dev/null
printf 'ensured Secret/%s\n' gavel-notify-database
apply_secret gavel-processes-temporal --from-literal=api-key="local-no-api-key-$(random_hex 8)"
