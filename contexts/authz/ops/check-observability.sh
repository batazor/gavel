#!/usr/bin/env bash
set -euo pipefail

rules=ops/Helm/files/prometheus/rules.yaml
dashboard=ops/Helm/files/grafana/dashboard.json
runbook=ops/Helm/README.md
telemetry=src/telemetry.rs

for metric in \
  gavel_authz_permission_checks_total \
  gavel_authz_permission_duration_micros_total \
  gavel_authz_receipts_total \
  gavel_authz_relationship_mutations_total
do
  grep -q "$metric" "$telemetry"
  grep -q "$metric" "$rules" "$dashboard"
done

for record in \
  gavel:authz:availability:ratio30d \
  gavel:authz:permission_requests:rate5m \
  gavel:authz:permission_dependency_failure_ratio:rate5m \
  gavel:authz:permission_average_latency_seconds:rate5m \
  gavel:authz:receipt_failure_ratio:rate15m
do
  grep -q "$record" "$rules"
  grep -q "$record" "$dashboard"
done

for alert in \
  AuthzUnavailable \
  AuthzPermissionDependencyFailuresHigh \
  AuthzPermissionLatencyHigh \
  AuthzReceiptFailuresHigh \
  AuthzReceiptRecoveryAbsent
do
  grep -q "$alert" "$rules"
  grep -q "$alert" "$runbook"
done
