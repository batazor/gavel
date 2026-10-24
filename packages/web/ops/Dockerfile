# syntax=docker/dockerfile:1.7
FROM node:22.22.0-alpine3.23 AS builder

WORKDIR /workspace
COPY package.json package-lock.json ./
COPY packages/ui/package.json packages/ui/package.json
COPY packages/web/package.json packages/web/package.json
RUN --mount=type=cache,target=/root/.npm npm ci

COPY packages/ui packages/ui
COPY packages/web packages/web

ARG VITE_DATA_MODE=connected
ARG VITE_GRAPHQL_URL=http://127.0.0.1:8080/graphql
ARG VITE_DEPLOYMENT_PROFILE=local
ARG VITE_LOCAL_AUTH_ENABLED=true
ARG VITE_LOCAL_AUTH_ISSUER=http://127.0.0.1:8080/default
ARG VITE_LOCAL_AUTH_AUDIENCE=gavel-web
ENV VITE_DATA_MODE=${VITE_DATA_MODE} \
    VITE_GRAPHQL_URL=${VITE_GRAPHQL_URL} \
    VITE_DEPLOYMENT_PROFILE=${VITE_DEPLOYMENT_PROFILE} \
    VITE_LOCAL_AUTH_ENABLED=${VITE_LOCAL_AUTH_ENABLED} \
    VITE_LOCAL_AUTH_ISSUER=${VITE_LOCAL_AUTH_ISSUER} \
    VITE_LOCAL_AUTH_AUDIENCE=${VITE_LOCAL_AUTH_AUDIENCE}
RUN npm run build --workspace=@gavel/web

FROM node:22.22.0-alpine3.23
WORKDIR /app
COPY packages/web/ops/server.mjs /app/server.mjs
COPY --from=builder /workspace/packages/web/dist /app/dist
ENV PORT=8080
EXPOSE 8080
USER node
ENTRYPOINT ["node", "/app/server.mjs"]
