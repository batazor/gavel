# syntax=docker/dockerfile:1.7
FROM rust:1.98-alpine3.24 AS builder

WORKDIR /build
COPY Cargo.toml ./
COPY src ./src

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release && \
    cp target/release/gavel-bff /tmp/service

FROM alpine:3.24 AS runtime

RUN apk add --no-cache ca-certificates && \
    addgroup -S -g 10001 gavel && \
    adduser -S -D -H -u 10001 -G gavel gavel

COPY --from=builder /tmp/service /usr/local/bin/service

USER 10001:10001
ENV PORT=8080
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD wget -q -O /dev/null "http://127.0.0.1:${PORT}/healthz" || exit 1

ENTRYPOINT ["/usr/local/bin/service"]
