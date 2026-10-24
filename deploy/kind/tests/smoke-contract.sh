#!/usr/bin/env bash
set -euo pipefail

SMOKE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/mvp-smoke.sh"
grep -q 'run_scenario "${happy_scenario}" success false' "${SMOKE}"
grep -q 'run_scenario "${expiry_scenario}" expire false' "${SMOKE}"
grep -q 'run_scenario "${restart_scenario}" expire true' "${SMOKE}"
grep -q 'rollout restart deployment/gavel-processes' "${SMOKE}"
grep -q '/local/sandbox/attempts/${auction}/1/expire' "${SMOKE}"
grep -q 'data.auction.status == "SOLD"' "${SMOKE}"
grep -q 'MVP_SEED_SCENARIO' "${SMOKE}"
grep -q 'outbid/${auction}:${takeover_version}' "${SMOKE}"
grep -q 'outbid/${auction}:${immediate_version}' "${SMOKE}"
grep -q 'assert_notification_missing' "${SMOKE}"
if grep -Eq '\bpsql\b|SELECT[[:space:]].*FROM|INSERT[[:space:]]+INTO' "${SMOKE}"; then
  printf 'smoke must use public/owner boundaries\n' >&2
  exit 1
fi
printf 'three isolated MVP smoke scenario contracts passed\n'
