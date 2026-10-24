#!/usr/bin/env bash
set -euo pipefail

chart="contexts/bff/ops/Helm"
local_values="contexts/bff/fixtures/local-auth/local-values.yaml"
production_values="contexts/bff/fixtures/local-auth/forbidden-production-values.yaml"
rendered="$(mktemp)"
trap 'rm -f "${rendered}"' EXIT

helm template gavel-bff "${chart}" -f "${local_values}" >"${rendered}"
grep -q 'envoy.filters.http.jwt_authn' "${rendered}"
grep -q 'issuer: http://127.0.0.1:8080/default' "${rendered}"
grep -q 'audiences: \[gavel-web\]' "${rendered}"
grep -q 'uri: http://local-jwt:8080/default/jwks' "${rendered}"
grep -q 'prefix: /graphql' "${rendered}"

if helm template gavel-bff "${chart}" -f "${production_values}" >/dev/null 2>&1; then
  printf 'production profile unexpectedly accepted local auth\n' >&2
  exit 1
fi

printf 'local JWT ingress render and production guard checks passed\n'
