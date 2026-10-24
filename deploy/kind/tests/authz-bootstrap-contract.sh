#!/usr/bin/env bash
set -euo pipefail

KIND_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${KIND_DIR}/../.." && pwd)"
VALUES="${KIND_DIR}/values/authz.yaml"
JOB="${REPO_ROOT}/contexts/authz/ops/Helm/templates/bootstrap-job.yaml"
CLIENT="${REPO_ROOT}/contexts/authz/src/local_bootstrap.rs"

grep -q '^  enabled: true$' "${VALUES}"
grep -q '^  localOnly: true$' "${VALUES}"
grep -q 'args: \["bootstrap"\]' "${JOB}"
grep -q 'AUTHZ_BOOTSTRAP_LOCAL_ONLY' "${JOB}"
grep -q 'register_principal' "${CLIENT}"
grep -q 'write_relationships' "${CLIENT}"
grep -q 'PermissionDecision::Denied' "${CLIENT}"
grep -q 'hook-delete-policy: before-hook-creation$' "${JOB}"
grep -q 'issuer: http://127.0.0.1:8080/default' "${VALUES}"
grep -q 'sync local JWT opaque principal claims' "${KIND_DIR}/scripts/identity.sh"

if grep -Eq 'SPICEDB_(TOKEN|CREDENTIAL)|preshared-key|datastore-uri' "${JOB}"; then
  printf 'Authz bootstrap Job must not receive raw SpiceDB credentials\n' >&2
  exit 1
fi

if grep -q 'spicedb_rpc' "${CLIENT}"; then
  printf 'Authz bootstrap client must use the owner API, not raw SpiceDB RPC\n' >&2
  exit 1
fi

printf 'Authz bootstrap owner-boundary contract passed\n'
