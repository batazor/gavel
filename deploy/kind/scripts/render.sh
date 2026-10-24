#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"
require_command kubectl
kubectl kustomize "${KIND_DIR}"
