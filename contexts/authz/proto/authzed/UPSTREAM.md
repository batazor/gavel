# Vendored Authzed API subset

Source: `https://github.com/authzed/api`

Revision: `3270b08244f3e97e2a57a7ee2d9c649c88b861d3`

License: Apache-2.0, copied in `LICENSE`.

The files under `api/v1/` are a source-compatible subset of the official
`authzed.api.v1` wire contract. Gavel keeps only the messages and RPCs used by
the Authz adapter, preserves their package, method names, field numbers and
enum values, and removes HTTP/OpenAPI/validation annotations plus unrelated
messages. This avoids a second Prost/Tonic ABI while keeping production builds
offline and reproducible.

To update the subset, compare each retained definition with the pinned upstream
revision, update this revision, run `buf format`, `buf lint`, `buf build`, and
the Authz SpiceDB integration suite.
