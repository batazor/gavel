# Authz operations

The chart runs three Authz replicas by default, exposes HTTP health/metrics on `8080` and gRPC on `9090`, and uses external Kubernetes Secrets. It never renders a Secret. The receipts migration and SpiceDB schema load are separate Helm hook Jobs so database ownership and rollout failures remain visible.

## Build and install

Run the image build from the repository root because Authz belongs to the Cargo workspace:

```bash
docker build -f contexts/authz/ops/Dockerfile -t ghcr.io/batazor/gavel-authz:dev .
helm upgrade --install authz contexts/authz/ops/Helm --namespace gavel --create-namespace
```

Before install, provision the Secrets named by `externalSecrets` in `values.yaml`: PostgreSQL URL, SpiceDB endpoint/credential, principal HMAC key, and receipt fingerprint key. The HMAC values must be independently generated 32-byte values and rotated deliberately; changing the principal key changes derived principal IDs.

## Endpoints and SLI

- `GET /healthz`: process liveness.
- `GET /readyz`: PostgreSQL and SpiceDB readiness.
- `GET /metrics`: bounded-label permission, receipt, and relationship metrics.
- gRPC `9090`: trusted-proxy-only Authz API; the proxy supplies sanitized SPIFFE context.

The dashboard UID is `gavel-authz`. Permission dependency health is measured by `dependency_failure` outcomes, latency by cumulative decision time divided by completed checks, and mutation durability by receipt outcomes. IDs, idempotency keys, credentials, and ZedTokens must never appear in labels or logs.

## Alert runbook

### AuthzUnavailable

Inspect ServiceMonitor targets, pod events, `/healthz`, and `/readyz`. Check PostgreSQL and SpiceDB connectivity before restarting or rolling back a correlated deployment.

### AuthzPermissionDependencyFailuresHigh

Split failures by permission operation, then inspect SpiceDB availability, deadlines, proxy connectivity, and schema-job status. Do not treat a domain deny as a dependency failure.

### AuthzPermissionLatencyHigh

Compare request rate with SpiceDB latency and saturation. Confirm client deadlines, network health, and whether bulk calls exceed their normal size. Preserve fail-closed behavior while mitigating.

### AuthzReceiptFailuresHigh

Separate `failed`, `conflict`, and `in_progress`. Check PostgreSQL locks, pool saturation, migration status, and caller idempotency behavior. Conflicts usually mean a caller reused a key for a different canonical payload.

### AuthzReceiptRecoveryAbsent

Inspect pending rows whose lease expired, worker health, PostgreSQL clock/locks, and the receipt lease configuration. Recovery is valid only for the same request fingerprint; never rewrite receipt rows by hand during diagnosis.

Resolve alerts only after their recording rules recover and attach dashboard, safe log, and trace evidence to the incident.
