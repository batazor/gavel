#!/usr/bin/env bash
set -euo pipefail

chart_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
rendered="$(helm template gavel-bff "${chart_dir}" -f "${chart_dir}/values-local.yaml")"

grep -q 'name: local-auth-ingress' <<<"${rendered}"
grep -q 'issuer: "http://127.0.0.1:8080/default"' <<<"${rendered}"
grep -q 'header_name: x-gavel-user-id' <<<"${rendered}"
grep -q 'header_name: x-gavel-principal' <<<"${rendered}"
grep -q 'key: x-correlation-id' <<<"${rendered}"
grep -q 'uri: "http://local-jwt:8080/default/jwks"' <<<"${rendered}"
grep -q 'provider_name: gavel_local' <<<"${rendered}"
grep -q 'type: NodePort' <<<"${rendered}"
grep -q 'nodePort: 30080' <<<"${rendered}"
grep -q 'name: envoy.filters.http.cors' <<<"${rendered}"
grep -q 'exact: "http://127.0.0.1:4173"' <<<"${rendered}"
grep -q 'string_match: { exact: "OPTIONS" }' <<<"${rendered}"
grep -q 'checksum/local-auth:' <<<"${rendered}"

if helm template gavel-bff "${chart_dir}" --set localAuth.enabled=true >/dev/null 2>&1; then
  echo 'production values unexpectedly enabled local auth' >&2
  exit 1
fi

printf 'BFF local ingress and production guard render correctly\n'
