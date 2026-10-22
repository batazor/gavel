#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/../../.." && pwd)"
telemetry="$root/contexts/catalog/src/telemetry.rs"
rules="$root/contexts/catalog/ops/Helm/files/prometheus/rules.yaml"
dashboard="$root/contexts/catalog/ops/Helm/files/grafana/dashboard.json"
runbook="$root/contexts/catalog/ops/Helm/README.md"

for metric in \
  gavel_catalog_operations_total \
  gavel_catalog_operation_duration_micros_total \
  gavel_catalog_pending_media_oldest_age_seconds \
  gavel_catalog_outbox_backlog \
  gavel_catalog_reconciliation_backlog; do
  rg -q "$metric" "$telemetry"
  rg -q "$metric|gavel:catalog:" "$rules"
  rg -q "$metric|gavel:catalog:" "$dashboard"
done

for topic in PendingMediaStuck OutboxBacklogHigh ReconciliationBacklogHigh; do
  rg -q "$topic" "$rules"
done
rg -q 'PendingMediaStuck' "$runbook"
rg -q 'OutboxBacklogHigh' "$runbook"
rg -q 'ReconciliationBacklogHigh' "$runbook"

if rg -n 'seller_id|lot_id|image_id|object_key|idempotency_key|consistency_token|credential' "$rules" "$dashboard"; then
  echo "private/high-cardinality label found in observability artifacts" >&2
  exit 1
fi
