#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"
DRY_RUN=false
[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=true

ensure_publication() {
  local database="$1"
  local publication="$2"
  local table="$3"
  local exists
  exists="$(kubectl_mvp exec statefulset/postgresql -- env PGPASSWORD="${admin_password}" \
    psql -v ON_ERROR_STOP=1 -U postgres -d "${database}" -Atc \
    "SELECT count(*) FROM pg_publication WHERE pubname = '${publication}'")"
  if [[ "${exists}" == "0" ]]; then
    kubectl_mvp exec statefulset/postgresql -- env PGPASSWORD="${admin_password}" \
      psql -v ON_ERROR_STOP=1 -U postgres -d "${database}" \
      -c "CREATE PUBLICATION ${publication} FOR TABLE ${table}" >/dev/null
  fi
  kubectl_mvp exec statefulset/postgresql -- env PGPASSWORD="${admin_password}" \
    psql -v ON_ERROR_STOP=1 -U postgres -d "${database}" \
    -c "ALTER PUBLICATION ${publication} SET TABLE ${table}" >/dev/null
}

if ! ${DRY_RUN}; then
  admin_password="$(kubectl_mvp get secret gavel-platform-postgres -o jsonpath='{.data.admin-password}' | base64 --decode)"
  ensure_publication gavel_catalog gavel_catalog_v1 public.catalog_outbox
  ensure_publication gavel_auction gavel_auction_public_v1 public.auction_outbox
  ensure_publication gavel_auction gavel_auction_restricted_v1 public.auction_process_outbox
  ensure_publication gavel_payments gavel_payments_v1 public.payment_outbox
fi

for template in "${KIND_DIR}"/connectors/*.json.tpl; do
  connector="gavel-$(basename "${template}" .json.tpl)-v1"
  if ${DRY_RUN}; then
    redacted_action "render and PUT connector ${connector} (database credential omitted)"
    continue
  fi
  cdc_password="$(kubectl_mvp get secret gavel-platform-postgres -o jsonpath='{.data.cdc-password}' | base64 --decode)"
  config="$(sed "s/@@CDC_PASSWORD@@/${cdc_password}/g" "${template}")"
  kubectl_mvp exec deployment/kafka-connect -- curl --fail --silent --show-error \
    -X PUT -H 'Content-Type: application/json' --data "${config}" \
    "http://localhost:8083/connectors/${connector}/config" >/dev/null
  printf 'reconciled connector %s\n' "${connector}"
done
