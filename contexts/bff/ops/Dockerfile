# syntax=docker/dockerfile:1.7
FROM node:22.22.0-alpine3.23 AS composer

WORKDIR /workspace
RUN npm install --global --ignore-scripts wgc@0.133.0

COPY contexts/bff/cosmo/supergraph.yaml contexts/bff/cosmo/supergraph.yaml
COPY contexts/catalog/graphql/schema.graphql contexts/catalog/graphql/schema.graphql
COPY contexts/auction/graphql/schema.graphql contexts/auction/graphql/schema.graphql
COPY contexts/payments/graphql/schema.graphql contexts/payments/graphql/schema.graphql

RUN mkdir -p /out && \
    wgc router compose \
      --input contexts/bff/cosmo/supergraph.yaml \
      --out /out/execution-config.json

FROM ghcr.io/wundergraph/cosmo/router:0.361.0-nonroot

COPY --from=composer /out/execution-config.json /etc/cosmo/execution-config.json
COPY contexts/bff/cosmo/router.yaml /etc/cosmo/router.yaml

ENV DEV_MODE=false \
    LISTEN_ADDR=0.0.0.0:8080 \
    EXECUTION_CONFIG_FILE_PATH=/etc/cosmo/execution-config.json
EXPOSE 8080
CMD ["/router", "-config", "/etc/cosmo/router.yaml"]
