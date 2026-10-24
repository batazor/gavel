#!/usr/bin/env bash
set -euo pipefail

SMOKE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/mvp-smoke.sh"
grep -q 'run_scenario happy success false' "${SMOKE}"
grep -q 'run_scenario expiry expire false' "${SMOKE}"
grep -q 'run_scenario restart expire true' "${SMOKE}"
grep -q 'rollout restart deployment/gavel-processes' "${SMOKE}"
grep -q '/local/sandbox/attempts/${auction}/1/expire' "${SMOKE}"
grep -q 'data.auction.status == "SOLD"' "${SMOKE}"
grep -q 'MVP_SEED_SCENARIO' "${SMOKE}"
if grep -Eq '\bpsql\b|SELECT[[:space:]].*FROM|INSERT[[:space:]]+INTO' "${SMOKE}"; then
  printf 'smoke must use public/owner boundaries\n' >&2
  exit 1
fi
printf 'three isolated MVP smoke scenario contracts passed\n'
