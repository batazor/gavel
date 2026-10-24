#!/bin/sh
set -eu

rendered=$(mktemp)
trap 'rm -f "$rendered"' EXIT
cat >"$rendered"

for pattern in 'kind: Deployment' 'kind: Service' 'kind: Job' 'name: DATABASE_URL' 'name: PAYMENT_FINGERPRINT_SECRET' 'name: PAYMENTS_RUNTIME_PROFILE' 'name: PAYMENT_PROVIDER_ADAPTER' 'path: /readyz' 'name: grpc'; do
  grep -F -- "$pattern" "$rendered" >/dev/null || {
    echo "missing rendered Payments contract: $pattern" >&2
    exit 1
  }
done

if grep -E 'postgres(ql)?://[^[:space:]]+:[^[:space:]]+@|PAYMENT_(PROVIDER|CALLBACK|FINGERPRINT|SANDBOX_CONTROL)_SECRET: [^<]' "$rendered" >/dev/null; then
  echo 'rendered Payments chart contains a materialized credential' >&2
  exit 1
fi
