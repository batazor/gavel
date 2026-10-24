#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"

CILIUM_VERSION="1.20.0"
ISTIO_VERSION="1.31.3"
CERT_MANAGER_VERSION="v1.21.2"
PROMETHEUS_OPERATOR_VERSION="v0.94.1"
PROMETHEUS_OPERATOR_BUNDLE="https://github.com/prometheus-operator/prometheus-operator/releases/download/${PROMETHEUS_OPERATOR_VERSION}/bundle.yaml"

if [[ "${1:-}" == "--dry-run" ]]; then
  redacted_action "helm upgrade --install cilium oci://quay.io/cilium/charts/cilium --version ${CILIUM_VERSION}"
  redacted_action "helm upgrade --install istio-base istio/base --version ${ISTIO_VERSION}"
  redacted_action "helm upgrade --install istiod istio/istiod --version ${ISTIO_VERSION}"
  redacted_action "helm upgrade --install cert-manager oci://quay.io/jetstack/charts/cert-manager --version ${CERT_MANAGER_VERSION}"
  redacted_action "kubectl apply prometheus-operator bundle ${PROMETHEUS_OPERATOR_VERSION}"
  exit 0
fi

assert_mvp_scope
require_command helm
require_command kubectl

if kubectl --context "${MVP_CONTEXT}" --namespace kube-system get daemonset kindnet >/dev/null 2>&1; then
  printf 'cluster %s uses kindnet; recreate it with deploy/kind/cluster.yaml before installing Cilium\n' \
    "${MVP_CLUSTER}" >&2
  exit 2
fi

helm upgrade --install cilium oci://quay.io/cilium/charts/cilium \
  --kube-context "${MVP_CONTEXT}" \
  --version "${CILIUM_VERSION}" \
  --namespace kube-system \
  --set image.pullPolicy=IfNotPresent \
  --set ipam.mode=kubernetes \
  --set operator.replicas=1 \
  --wait \
  --timeout 10m

kubectl --context "${MVP_CONTEXT}" wait \
  --for=condition=Ready nodes --all --timeout=5m

helm repo add istio https://blob.istio.io/istio-release/charts --force-update
helm upgrade --install istio-base istio/base \
  --kube-context "${MVP_CONTEXT}" \
  --version "${ISTIO_VERSION}" \
  --namespace istio-system \
  --create-namespace \
  --set defaultRevision=default \
  --wait \
  --timeout 10m
helm upgrade --install istiod istio/istiod \
  --kube-context "${MVP_CONTEXT}" \
  --version "${ISTIO_VERSION}" \
  --namespace istio-system \
  --wait \
  --timeout 10m

helm upgrade --install cert-manager oci://quay.io/jetstack/charts/cert-manager \
  --kube-context "${MVP_CONTEXT}" \
  --version "${CERT_MANAGER_VERSION}" \
  --namespace cert-manager \
  --create-namespace \
  --set crds.enabled=true \
  --set prometheus.enabled=false \
  --wait \
  --timeout 10m

kubectl --context "${MVP_CONTEXT}" apply --server-side \
  --field-manager=gavel-kind-platform \
  -f "${PROMETHEUS_OPERATOR_BUNDLE}"
kubectl --context "${MVP_CONTEXT}" --namespace default wait \
  --for=condition=Available deployment/prometheus-operator --timeout=5m

kubectl --context "${MVP_CONTEXT}" get crd \
  ciliumnetworkpolicies.cilium.io \
  authorizationpolicies.security.istio.io \
  servicemonitors.monitoring.coreos.com \
  prometheusrules.monitoring.coreos.com \
  certificates.cert-manager.io \
  issuers.cert-manager.io >/dev/null

printf 'kind platform is ready: Cilium %s, Istio %s, cert-manager %s, Prometheus Operator %s\n' \
  "${CILIUM_VERSION}" "${ISTIO_VERSION}" "${CERT_MANAGER_VERSION}" "${PROMETHEUS_OPERATOR_VERSION}"
