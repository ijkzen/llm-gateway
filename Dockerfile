#check=skip=SecretsUsedInArgOrEnv

ARG BUILD_IMAGE=192.168.31.100:2080/ijkzen/build-rs:v0.5
ARG RUNTIME_IMAGE=192.168.31.100:2080/ijkzen/base-ffmpeg:v0.8

ARG SCCACHE_BACKEND=s3://rust_build_cache/llm-gateway/linux/amd64
ARG SCCACHE_S3_ENDPOINT=http://192.168.31.100:2041
ARG SCCACHE_S3_USE_SSL=false
ARG SCCACHE_REGION=us-east-1
ARG AWS_ACCESS_KEY_ID=admin
ARG AWS_SECRET_ACCESS_KEY=password

FROM node:22-slim AS web-builder

WORKDIR /app
COPY web/package.json web/pnpm-lock.yaml web/.npmrc web/pnpm-workspace.yaml ./
RUN corepack enable && pnpm install --frozen-lockfile
COPY web ./
RUN pnpm build

FROM ${BUILD_IMAGE} AS planner

WORKDIR /app

ARG SCCACHE_BACKEND
ARG SCCACHE_S3_ENDPOINT
ARG SCCACHE_S3_USE_SSL
ARG SCCACHE_REGION
ARG AWS_ACCESS_KEY_ID
ARG AWS_SECRET_ACCESS_KEY

ENV SCCACHE_BACKEND=${SCCACHE_BACKEND}
ENV SCCACHE_S3_ENDPOINT=${SCCACHE_S3_ENDPOINT}
ENV SCCACHE_ENDPOINT=${SCCACHE_S3_ENDPOINT}
ENV SCCACHE_S3_USE_SSL=${SCCACHE_S3_USE_SSL}
ENV SCCACHE_REGION=${SCCACHE_REGION}
ENV AWS_ACCESS_KEY_ID=${AWS_ACCESS_KEY_ID}
ENV AWS_SECRET_ACCESS_KEY=${AWS_SECRET_ACCESS_KEY}

COPY Cargo.toml Cargo.lock ./
COPY .cargo ./.cargo
COPY scripts ./scripts
COPY src ./src

RUN chmod +x /app/scripts/sccache-env.sh /app/scripts/rustc-wrapper.sh
RUN cargo chef prepare --recipe-path recipe.json

FROM ${BUILD_IMAGE} AS rust-deps

WORKDIR /app

ARG SCCACHE_BACKEND
ARG SCCACHE_S3_ENDPOINT
ARG SCCACHE_S3_USE_SSL
ARG SCCACHE_REGION
ARG AWS_ACCESS_KEY_ID
ARG AWS_SECRET_ACCESS_KEY

ENV SCCACHE_BACKEND=${SCCACHE_BACKEND}
ENV SCCACHE_S3_ENDPOINT=${SCCACHE_S3_ENDPOINT}
ENV SCCACHE_ENDPOINT=${SCCACHE_S3_ENDPOINT}
ENV SCCACHE_S3_USE_SSL=${SCCACHE_S3_USE_SSL}
ENV SCCACHE_REGION=${SCCACHE_REGION}
ENV AWS_ACCESS_KEY_ID=${AWS_ACCESS_KEY_ID}
ENV AWS_SECRET_ACCESS_KEY=${AWS_SECRET_ACCESS_KEY}

COPY Cargo.toml Cargo.lock ./
COPY .cargo ./.cargo
COPY scripts ./scripts
COPY --from=planner /app/recipe.json ./recipe.json

RUN chmod +x /app/scripts/sccache-env.sh /app/scripts/rustc-wrapper.sh
RUN cargo chef cook --release --recipe-path recipe.json

FROM ${BUILD_IMAGE} AS builder

WORKDIR /app

ARG SCCACHE_BACKEND
ARG SCCACHE_S3_ENDPOINT
ARG SCCACHE_S3_USE_SSL
ARG SCCACHE_REGION
ARG AWS_ACCESS_KEY_ID
ARG AWS_SECRET_ACCESS_KEY

ENV SCCACHE_BACKEND=${SCCACHE_BACKEND}
ENV SCCACHE_S3_ENDPOINT=${SCCACHE_S3_ENDPOINT}
ENV SCCACHE_ENDPOINT=${SCCACHE_S3_ENDPOINT}
ENV SCCACHE_S3_USE_SSL=${SCCACHE_S3_USE_SSL}
ENV SCCACHE_REGION=${SCCACHE_REGION}
ENV AWS_ACCESS_KEY_ID=${AWS_ACCESS_KEY_ID}
ENV AWS_SECRET_ACCESS_KEY=${AWS_SECRET_ACCESS_KEY}

COPY Cargo.toml Cargo.lock ./
COPY .cargo ./.cargo
COPY scripts ./scripts
COPY src ./src
COPY --from=rust-deps /app/target ./target
COPY --from=web-builder /app/dist ./web/dist
RUN test -d /app/web/dist && test -f /app/web/dist/index.html

RUN chmod +x /app/scripts/sccache-env.sh /app/scripts/rustc-wrapper.sh
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
