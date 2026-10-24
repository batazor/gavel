#!/bin/sh
set -eu

rendered=$(mktemp)
trap 'rm -f "$rendered"' EXIT
cat >"$rendered"

for pattern in 'kind: Deployment' 'kind: Service' 'kind: Job' 'name: DATABASE_URL' 'name: COMMAND_FINGERPRINT_KEY_HEX' 'name: REDIS_URL' 'name: REDPANDA_BROKERS' 'name: REDPANDA_SASL_USERNAME' 'gavel-redpanda-auction' 'path: /readyz' 'name: grpc'; do
  grep -F -- "$pattern" "$rendered" >/dev/null || {
    echo "missing rendered Auction contract: $pattern" >&2
    exit 1
  }
done

if grep -E 'postgres(ql)?://[^[:space:]]+:[^[:space:]]+@|COMMAND_FINGERPRINT_KEY_HEX: [^<]' "$rendered" >/dev/null; then
  echo 'rendered Auction chart contains a materialized credential' >&2
  exit 1
fi
