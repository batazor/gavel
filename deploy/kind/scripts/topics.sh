#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"
DRY_RUN=false
[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=true

process_topics=(gavel.auction.public.v1 gavel.auction.processes.v1 gavel.payments.attempts.v1)
auction_topics=(gavel.catalog.events.v1 gavel.payments.instruments.v1)
source_topics=("${process_topics[@]}" "${auction_topics[@]}")
quarantine_topic=gavel.processes.quarantine.v1
topics=("${source_topics[@]}" "${quarantine_topic}")
if ${DRY_RUN}; then
  redacted_action "ensure broker users gavel-connect, gavel-processes and gavel-auction"
  for topic in "${topics[@]}"; do redacted_action "ensure topic ${topic} (3 partitions, replication factor 1)"; done
  redacted_action "ensure least-privilege producer/consumer/group ACLs (credentials omitted)"
  exit 0
fi

admin_user="$(kubectl_mvp get secret gavel-redpanda-admin -o jsonpath='{.data.username}' | base64 --decode)"
admin_password="$(kubectl_mvp get secret gavel-redpanda-admin -o jsonpath='{.data.password}' | base64 --decode)"
connect_password="$(kubectl_mvp get secret gavel-redpanda-connect -o jsonpath='{.data.password}' | base64 --decode)"
processes_password="$(kubectl_mvp get secret gavel-redpanda-processes -o jsonpath='{.data.password}' | base64 --decode)"
auction_password="$(kubectl_mvp get secret gavel-redpanda-auction -o jsonpath='{.data.password}' | base64 --decode)"
kubectl_mvp exec statefulset/redpanda -- \
  rpk cluster config set superusers "[\"${admin_user}\"]" >/dev/null
rpk=(kubectl_mvp exec statefulset/redpanda -- rpk -X brokers=redpanda:9092 -X user="${admin_user}" -X pass="${admin_password}" -X sasl.mechanism=SCRAM-SHA-256)

"${rpk[@]}" security user create gavel-connect --password "${connect_password}" --mechanism SCRAM-SHA-256 2>/dev/null || true
"${rpk[@]}" security user create gavel-processes --password "${processes_password}" --mechanism SCRAM-SHA-256 2>/dev/null || true
"${rpk[@]}" security user create gavel-auction --password "${auction_password}" --mechanism SCRAM-SHA-256 2>/dev/null || true
for topic in "${topics[@]}"; do
  if ! "${rpk[@]}" topic describe "${topic}" >/dev/null 2>&1; then
    "${rpk[@]}" topic create "${topic}" --partitions 3 --replicas 1
  fi
done
for topic in "${topics[@]}"; do
  "${rpk[@]}" acl create --allow-principal User:gavel-connect --operation write,describe --topic "${topic}"
done
for topic in "${process_topics[@]}"; do
  "${rpk[@]}" acl create --allow-principal User:gavel-processes --operation read,describe --topic "${topic}" --group gavel-processes-v1
done
for topic in "${auction_topics[@]}"; do
  "${rpk[@]}" acl create --allow-principal User:gavel-auction --operation read,describe --topic "${topic}" --group gavel-auction-owner-events-v1
done
"${rpk[@]}" acl create --allow-principal User:gavel-connect --operation all --topic _gavel_connect_ --resource-pattern-type prefixed
"${rpk[@]}" acl create --allow-principal User:gavel-connect --operation all --group gavel-debezium-connect-v1
"${rpk[@]}" acl create --allow-principal User:gavel-processes --operation write,describe --topic "${quarantine_topic}"
