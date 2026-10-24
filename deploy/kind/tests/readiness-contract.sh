#!/usr/bin/env bash
set -euo pipefail

KIND_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
READY="${KIND_DIR}/scripts/ready.sh"
DIAGNOSTICS="${KIND_DIR}/scripts/diagnostics.sh"

grep -q 'health/ready' "${READY}"
grep -q 'gavel-processes-gavel-processes' "${READY}"
grep -q 'connectors/${connector}/status' "${READY}"
grep -q 'tasks\[\].state' "${READY}"
grep -q 'api/v1/info' "${READY}"
grep -q 'endpointslice' "${READY}"
grep -q 'lots(first: 1)' "${READY}"
grep -q '\[REDACTED\]' "${DIAGNOSTICS}"
grep -q 'maximum\[_-\]?amount' "${DIAGNOSTICS}"
grep -q 'recovery\[_-\]?url' "${DIAGNOSTICS}"

printf 'MVP readiness and sanitized failure-bundle contract passed\n'
