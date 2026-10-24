#!/bin/sh
set -eu

rendered=$(mktemp)
trap 'rm -f "$rendered"' EXIT
cat >"$rendered"

for pattern in 'kind: Deployment' 'kind: Service' 'kind: Job' 'name: CATALOG_DATABASE_URL' 'name: CATALOG_S3_ENDPOINT' 'name: CATALOG_FINGERPRINT_HMAC_KEY' 'path: /readyz' 'name: grpc'; do
  grep -F -- "$pattern" "$rendered" >/dev/null || {
    echo "missing rendered Catalog contract: $pattern" >&2
    exit 1
  }
done

if grep -E 'postgres(ql)?://[^[:space:]]+:[^[:space:]]+@|CATALOG_S3_SECRET_KEY: [^<]' "$rendered" >/dev/null; then
  echo 'rendered Catalog chart contains a materialized credential' >&2
  exit 1
fi
