#!/bin/sh
set -eu

rendered=$(mktemp)
trap 'rm -f "$rendered"' EXIT
cat >"$rendered"

required='kind: Deployment
kind: Job
name: DATABASE_URL
name: SMTP_ADDR
name: PROCESSES_SPIFFE_ID
/gavel.notify.v1.NotifyService/SendNotification
/gavel.notify.v1.NotifyOperatorService/GetNotification
path: /readyz'

echo "$required" | while IFS= read -r pattern; do
  grep -F -- "$pattern" "$rendered" >/dev/null || {
    echo "missing rendered Notify contract: $pattern" >&2
    exit 1
  }
done

if grep -E '(postgres(ql)?://[^[:space:]]+:[^[:space:]]+@|SMTP_PASSWORD|recovery_url)' "$rendered" >/dev/null; then
  echo "rendered Notify chart contains a credential or private payload field" >&2
  exit 1
fi
