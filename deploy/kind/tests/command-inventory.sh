#!/usr/bin/env bash
set -euo pipefail

kind_dir="$(cd "$(dirname "$0")/.." && pwd)"
repo_root="$(cd "${kind_dir}/../.." && pwd)"
commands=(mvp-platform mvp-up mvp-down mvp-seed mvp-ready mvp-smoke mvp-e2e mvp-diagnostics)

for command in "${commands[@]}"; do
  help_output="$(make --no-print-directory -C "${repo_root}" help)"
  grep -Fq "${command}" <<<"${help_output}" || {
    printf 'make help does not expose %s\n' "${command}" >&2
    exit 1
  }
  dry_run="$(make --no-print-directory -C "${repo_root}" -n "${command}")"
  phase="${command#mvp-}"
  grep -Fq "deploy/kind/scripts/mvp.sh ${phase}" <<<"${dry_run}" || {
    printf '%s does not dispatch through the pinned MVP orchestrator\n' "${command}" >&2
    exit 1
  }
done

MVP_CLUSTER=gavel-mvp MVP_NAMESPACE=gavel-mvp MVP_CONTEXT=kind-gavel-mvp \
  "${kind_dir}/scripts/mvp.sh" --check >/dev/null

if MVP_CLUSTER=another-cluster "${kind_dir}/scripts/mvp.sh" --check >/dev/null 2>&1; then
  printf 'dispatcher accepted an unsafe cluster override\n' >&2
  exit 1
fi
if MVP_NAMESPACE=another-namespace "${kind_dir}/scripts/mvp.sh" --check >/dev/null 2>&1; then
  printf 'dispatcher accepted an unsafe namespace override\n' >&2
  exit 1
fi
if MVP_CONTEXT=another-context "${kind_dir}/scripts/mvp.sh" --check >/dev/null 2>&1; then
  printf 'dispatcher accepted an unsafe context override\n' >&2
  exit 1
fi

cluster_name="$(awk '/^name: / {print $2; exit}' "${kind_dir}/cluster.yaml")"
inventory_cluster="$(awk '/^    name: / {print $2; exit}' "${kind_dir}/inventory.yaml")"
[[ "${cluster_name}" == "gavel-mvp" && "${inventory_cluster}" == "gavel-mvp" ]] || {
  printf 'cluster inventory is not pinned to gavel-mvp\n' >&2
  exit 1
}

printf 'MVP command inventory and explicit target guards passed\n'
