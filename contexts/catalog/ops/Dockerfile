# syntax=docker/dockerfile:1.7
FROM rust:1.85-slim-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY contexts ./contexts

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --locked --release -p gavel-catalog && \
    cp target/release/gavel-catalog /tmp/service

FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --gid 10001 gavel && \
    useradd --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin gavel

COPY --from=builder /tmp/service /usr/local/bin/service

USER 10001:10001
ENV CATALOG_HTTP_ADDR=0.0.0.0:8080 CATALOG_GRPC_ADDR=0.0.0.0:9090
EXPOSE 8080 9090
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD curl --fail --silent "http://127.0.0.1:8080/healthz" >/dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/service"]
