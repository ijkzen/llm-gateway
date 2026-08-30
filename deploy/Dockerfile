# 运行时镜像：仅打包本地 zig 交叉编译出的静态二进制（x86_64-unknown-linux-musl）。
# 构建前确保 deploy/llm-gateway 存在：
#   cargo zigbuild --release --target x86_64-unknown-linux-musl
#   cp target/x86_64-unknown-linux-musl/release/llm-gateway deploy/
FROM alpine:3.21

# tzdata 提供 /usr/share/zoneinfo（alpine 默认不含），TZ 才能生效，
# 否则 chrono::Local 静默回退 UTC，cron 表达式与前端东八区展示差 8 小时。
RUN apk add --no-cache tzdata \
    && adduser -D -u 10001 app \
    && mkdir -p /config/db /config/logs \
    && chown -R app:app /config

# cron 表达式按服务器本地时区解释（src/cron/parser.rs 使用 chrono::Local），
# 设置东八区与前端浏览器展示一致；东八区无 DST，无 tokio-cron-scheduler 时区快照问题。
ENV TZ=Asia/Shanghai

COPY llm-gateway /usr/local/bin/llm-gateway

USER app
EXPOSE 4007
ENTRYPOINT ["llm-gateway"]
