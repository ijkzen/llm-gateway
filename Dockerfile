ARG BUILD_IMAGE=192.168.31.100:2080/ijkzen/build-rs:v0.5
ARG RUNTIME_IMAGE=192.168.31.100:2080/ijkzen/base-ffmpeg:v0.8

FROM node:22-slim AS web-builder

WORKDIR /app
COPY web/package.json web/pnpm-lock.yaml web/.npmrc web/pnpm-workspace.yaml ./
RUN corepack enable && pnpm install --frozen-lockfile
COPY web ./
RUN pnpm build

FROM ${BUILD_IMAGE} AS planner

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo chef prepare --recipe-path recipe.json

FROM ${BUILD_IMAGE} AS rust-deps

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY --from=planner /app/recipe.json ./recipe.json

RUN cargo chef cook --release --recipe-path recipe.json

FROM ${BUILD_IMAGE} AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY --from=rust-deps /app/target ./target
COPY --from=web-builder /app/dist ./web/dist
RUN test -d /app/web/dist && test -f /app/web/dist/index.html

RUN cargo build --release

FROM ${RUNTIME_IMAGE} AS runtime

WORKDIR /app

ENV release=1
ENV APP_ENV=prod

COPY --from=builder /app/target/release/llm-gateway /app/llm-gateway

RUN mkdir -p /config/db /config/logs && chmod 755 /config/db /config/logs
RUN chmod +x /app/llm-gateway

# The runtime base image may not include curl, which the HEALTHCHECK needs.
# Try Debian/Ubuntu first, then Alpine, to cover the most common base images.
RUN DEBIAN_FRONTEND=noninteractive apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends curl && \
    rm -rf /var/lib/apt/lists/* || \
    apk add --no-cache curl

EXPOSE 4007

HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
  CMD curl -fsS --max-time 5 http://localhost:4007/api/healthz || exit 1

CMD ["/app/llm-gateway"]
