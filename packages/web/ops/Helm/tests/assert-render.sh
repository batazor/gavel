#!/usr/bin/env bash
set -euo pipefail
chart_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
rendered="$(helm template gavel-web "${chart_dir}" -f "${chart_dir}/values-local.yaml")"
grep -q 'image: "gavel-web:local"' <<<"${rendered}"
grep -q 'path: /readyz' <<<"${rendered}"
grep -q 'port: 8080' <<<"${rendered}"
printf 'Web local image, probes and port render correctly\n'
