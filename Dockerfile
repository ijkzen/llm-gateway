# llm-gateway 多阶段构建镜像（全公网依赖，GitHub Actions 可直接构建）
#
# 阶段 1：构建前端（Vite + React），产物 web/dist
FROM node:22-slim AS web-builder

WORKDIR /app
COPY web/package.json web/pnpm-lock.yaml web/.npmrc web/pnpm-workspace.yaml ./
RUN corepack enable && pnpm install --frozen-lockfile
COPY web ./
RUN pnpm build

# 阶段 2：cargo-chef 分析依赖
FROM rust:1-slim AS planner

WORKDIR /app

RUN cargo install cargo-chef --locked

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo chef prepare --recipe-path recipe.json

# 阶段 3：预编译依赖
FROM rust:1-slim AS rust-deps

WORKDIR /app

RUN cargo install cargo-chef --locked

COPY Cargo.toml Cargo.lock ./
COPY --from=planner /app/recipe.json ./recipe.json

RUN cargo chef cook --release --recipe-path recipe.json

# 阶段 4：正式构建（内嵌前端 dist）
FROM rust:1-slim AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY --from=rust-deps /app/target ./target
COPY --from=web-builder /app/dist ./web/dist
RUN test -d /app/web/dist && test -f /app/web/dist/index.html

RUN cargo build --release

# 阶段 5：运行时（debian slim；rustls 纯 Rust TLS，无 OpenSSL 系统依赖）
FROM debian:bookworm-slim AS runtime

WORKDIR /app

ENV release=1
ENV APP_ENV=prod

# healthcheck 需要 curl（debian slim 默认没有）；建非 root 运行用户
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl passwd \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 app \
    && mkdir -p /config/db /config/logs \
    && chown -R app:app /config

COPY --from=builder /app/target/release/llm-gateway /app/llm-gateway

USER app

EXPOSE 4007

HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
  CMD curl -fsS --max-time 5 http://localhost:4007/api/healthz || exit 1

CMD ["/app/llm-gateway"]
