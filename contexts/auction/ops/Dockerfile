# syntax=docker/dockerfile:1.7
FROM rust:1.98-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
      cmake g++ make pkg-config libsasl2-dev libssl-dev libcurl4-openssl-dev && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY .cargo ./.cargo
COPY contexts ./contexts

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked -p gavel-auction && \
    cp target/release/gavel-auction /tmp/service

FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl libsasl2-2 libssl3 && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --gid 10001 gavel && \
    useradd --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin gavel

COPY --from=builder /tmp/service /usr/local/bin/service

USER 10001:10001
ENV PORT=8080
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD curl --fail --silent "http://127.0.0.1:${PORT}/healthz" >/dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/service"]
