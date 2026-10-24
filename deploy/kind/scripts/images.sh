#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"

DRY_RUN=false
[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=true

assert_mvp_scope

if ! ${DRY_RUN}; then
  require_command docker
  require_command kind
fi

images=(gavel-catalog gavel-auction gavel-payments gavel-authz gavel-processes gavel-notify gavel-bff gavel-web)
dockerfiles=(contexts/catalog/ops/Dockerfile contexts/auction/ops/Dockerfile contexts/payments/ops/Dockerfile contexts/authz/ops/Dockerfile contexts/processes/ops/Dockerfile contexts/notify/ops/Dockerfile contexts/bff/ops/Dockerfile packages/web/ops/Dockerfile)
contexts=(. . . . . . . .)

for index in "${!images[@]}"; do
  image="${images[$index]}:local"
  dockerfile="${REPO_ROOT}/${dockerfiles[$index]}"
  context="${REPO_ROOT}/${contexts[$index]}"
  if ${DRY_RUN}; then
    redacted_action "build and load ${image} from ${dockerfiles[$index]}"
    continue
  fi
  docker buildx build --load --provenance=false --file "${dockerfile}" --tag "${image}" "${context}"
  local_image_id="$(docker image inspect --format '{{.Id}}' "${image}")"
  needs_load=false
  while IFS= read -r node; do
    node_image_id="$(docker exec "${node}" ctr --namespace k8s.io images ls \
      "name==docker.io/library/${image}" 2>/dev/null | awk 'NR == 2 {print $3}')"
    if [[ "${node_image_id}" != "${local_image_id}" ]]; then
      docker exec "${node}" crictl rmi "${image}" >/dev/null 2>&1 || true
      needs_load=true
    fi
  done < <(kind get nodes --name "${MVP_CLUSTER}")
  if ${needs_load}; then
    kind load docker-image --name "${MVP_CLUSTER}" "${image}"
  else
    printf 'reuse kind image %s (%s)\n' "${image}" "${local_image_id}"
  fi
done
