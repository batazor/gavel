#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
grep -q 'make mvp-up' "${ROOT}/README.md"
grep -q 'make mvp-platform' "${ROOT}/deploy/kind/README.md"
grep -q 'make mvp-smoke' "${ROOT}/README.md"
grep -q 'make mvp-e2e' "${ROOT}/README.md"
grep -q 'gavel-local-bidder-one' "${ROOT}/deploy/kind/README.md"
grep -q 'recovery_then_success' "${ROOT}/deploy/kind/README.md"
grep -q 'make mvp-diagnostics' "${ROOT}/deploy/kind/README.md"
grep -q "environment 'Local MVP (kind)'" "${ROOT}/docs/architecture/deployment.c4"
printf 'MVP operator documentation contract passed\n'
