# syntax=docker/dockerfile:1.7
FROM rust:1.85-bookworm AS builder

WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY .cargo ./.cargo
COPY contexts ./contexts

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/workspace/target \
    cargo build --locked --release -p gavel-authz && \
    cp target/release/gavel-authz /tmp/gavel-authz

FROM debian:bookworm-slim AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates wget && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --system --gid 10001 gavel && \
    useradd --system --uid 10001 --gid gavel --no-create-home gavel

COPY --from=builder /tmp/gavel-authz /usr/local/bin/gavel-authz

USER 10001:10001
EXPOSE 8080 9090
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD wget -q -O /dev/null http://127.0.0.1:8080/healthz || exit 1

ENTRYPOINT ["/usr/local/bin/gavel-authz"]
