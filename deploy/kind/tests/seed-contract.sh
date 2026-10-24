#!/usr/bin/env bash
set -euo pipefail

KIND_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SEED="${KIND_DIR}/scripts/seed.sh"

grep -q 'requestLotImageUpload' "${SEED}"
grep -q 'completeLotImageUpload' "${SEED}"
grep -q 'listLot' "${SEED}"
grep -q 'beginPaymentInstrumentRegistration' "${SEED}"
grep -q '/local/sandbox/instruments/' "${SEED}"
grep -q 'data.auction.status == "OPEN"' "${SEED}"
grep -q 'mvp-seed-lot-${seed_key}' "${SEED}"
grep -q 'MVP_SEED_SCENARIO' "${SEED}"
grep -q 'clusterUid' "${SEED}"

if grep -Eq '\bpsql\b|INSERT[[:space:]]+INTO|UPDATE[[:space:]].*SET' "${SEED}"; then
  printf 'seed must use public/owner boundaries, not direct database writes\n' >&2
  exit 1
fi

printf 'MVP seed boundary contract passed\n'
