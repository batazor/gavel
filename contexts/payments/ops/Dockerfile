# syntax=docker/dockerfile:1.7
FROM rust:1.85.1-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY .cargo ./.cargo
COPY contexts ./contexts
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --locked --release -p gavel-payments && \
    cp target/release/gavel-payments /tmp/gavel-payments

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --gid 10001 gavel && useradd --uid 10001 --gid gavel --no-create-home gavel
COPY --from=builder /tmp/gavel-payments /usr/local/bin/gavel-payments
USER 10001:10001
ENV HTTP_ADDR=0.0.0.0:8080 GRPC_ADDR=0.0.0.0:9090
EXPOSE 8080 9090
ENTRYPOINT ["/usr/local/bin/gavel-payments"]
