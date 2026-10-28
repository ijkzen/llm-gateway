# 部署、CI 与安全

## CI/CD（`.github/workflows/`）

- `ci.yml`：push 到 main / PR 时运行测试、clippy、fmt（先构建前端）。
- `nightly.yml`：每次 push 到 main 用 `docker buildx` 构建镜像并推送到 **Nightly 渠道**（`ghcr.io/ijkzen/llm-gateway:nightly`，覆盖式），供尝鲜，不建 Release。
- `release.yml`：**只匹配 `v*` tag**，正式发布渠道。流程：`bash scripts/check-release-version.sh` 校验 `Cargo.toml` 与 `web/package.json` 版本等于 tag 版本 → `cargo test` + `pnpm vitest run` → 构建推送 `ghcr.io/ijkzen/llm-gateway:vX.Y.Z`（同时更新 `:latest`）→ `softprops/action-gh-release` 创建 GitHub Release 页（自动 changelog）。

## 发布 ≠ 本地部署

两条互不相干的流程：

- **正式发布**：改版本号 + 打 `v*` tag + push，走 release.yml，只涉及 GitHub。操作步骤见 `.agents/skills/release-management/SKILL.md`。
- **FRP/阿里云本地部署**：zig 交叉编译 + `.deploy/deploy.sh`，镜像 tag 固定 `llm-gateway:latest`，不读不写版本号。见 `.agents/skills/local-deploy/SKILL.md`。

## 生产容器

- 暴露端口 `4007`。
- 需挂载 `/config/db` 与 `/config/logs` 以保证数据与日志持久化。
- 运行时镜像（根 `Dockerfile` 的 debian 层与 `deploy/Dockerfile` 的 alpine 层）安装了 tzdata 并设置 `TZ=Asia/Shanghai`（cron 表达式按东八区解释，见 `cron.md`）；compose 文件的 `TZ` 环境变量作为运行时兜底。`.deploy/deploy.sh` 部署后验证容器时区为 `CST`。

## 安全

- **认证**：管理接口（`/api/*`，除 auth/status|login|init 与 healthz）需要 Cookie Session；`/v1/*` 需要 Bearer API Key（api_key 表校验）。密码 argon2id 哈希，session 表只存令牌摘要。SPA 静态资源不做服务端拦截（前端路由守卫负责跳转）。
- **CORS**：`middleware/mod.rs` 使用 `CorsLayer::permissive()`，允许任意来源跨域访问 API（包括浏览器中任意网页）。已登记风险，部署在不可信网络前务必收敛。Cookie 为 SameSite=Lax，跨站请求不会携带会话。
- **Panic 处理**：通过 `CatchPanicLayer` 捕获 panic，返回 HTTP 500 与中文提示「服务器内部错误」。
- **敏感信息**：Dockerfile 与 CI 工作流中硬编码了内部镜像仓库地址、S3 端点与访问密钥，生产部署前应改为 Secret/环境变量注入。
- **文件权限**：生产数据库目录 `/config/db` 与日志目录 `/config/logs` 在 Dockerfile 中创建并设置 `755`。
- **输入校验**：HTTP 层做基础校验（Cron 表达式可解析、设置值按声明类型校验），关键业务逻辑由调度器内部再次校验。
