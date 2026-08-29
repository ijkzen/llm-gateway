# 运行时镜像：仅打包本地 zig 交叉编译出的静态二进制（x86_64-unknown-linux-musl）。
# 构建前确保 deploy/llm-gateway 存在：
#   cargo zigbuild --release --target x86_64-unknown-linux-musl
#   cp target/x86_64-unknown-linux-musl/release/llm-gateway deploy/
FROM alpine:3.21

RUN adduser -D -u 10001 app \
    && mkdir -p /config/db /config/logs \
    && chown -R app:app /config

COPY llm-gateway /usr/local/bin/llm-gateway

USER app
EXPOSE 4007
ENTRYPOINT ["llm-gateway"]
