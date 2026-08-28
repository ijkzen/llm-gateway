# 最佳实践审计整改决策跟踪文档

本文档用于跟踪对 `rs-template` 项目最佳实践审计问题的用户确认状态与整改状态。

## 说明

- 状态：`pending`（待确认） / `confirmed`（确认修改） / `skipped`（跳过不改） / `needs-info`（需要更多信息） / `done`（已完成）
- 每个问题需要用户通过 `AskUserQuestion` 确认是否修复、如何修复。
- 确认后由子代理按文档逐项修改，完成一项标记一项。

---

## P0 - 立即处理（高风险）

| # | 分类 | 问题摘要 | 位置 | 风险 | 状态 | 用户决策 | 备注 |
|---|------|----------|------|------|------|----------|------|
| P0-1 | 安全 | Dockerfile 与 CI 中硬编码 S3/Harbor 凭据 | `Dockerfile:6-11`, `build.yaml:35-36` | 高 | skipped | 跳过不改 | |
| P0-2 | 安全 | Dockerfile 第一行 `#check=skip=SecretsUsedInArgOrEnv` 绕过安全检查 | `Dockerfile:1` | 高 | skipped | 跳过不改 | |
| P0-3 | 安全 | CORS 完全开放 `CorsLayer::permissive()` | `src/middleware/mod.rs:14` | 高 | skipped | 跳过不改 | |
| P0-4 | 安全 | 无身份认证/授权，管理后台完全开放 | `src/routes/mod.rs:14-18` | 高 | skipped | 跳过不改 | |
| P0-5 | 健壮性 | async 中调用阻塞 I/O（`ensure_sqlite_dir`） | `src/db.rs:7-24` | 高 | confirmed | 立即修复，改用 tokio::fs | |
| P0-6 | 健壮性 | async 中调用阻塞 I/O（`setup_logging` 创建目录） | `src/lib.rs:37` | 高 | confirmed | 立即修复，setup_logging 返回 anyhow::Result | |
| P0-7 | 健壮性 | 日志清理任务使用阻塞 I/O | `src/logs_cleanup.rs:5-25` | 高 | confirmed | 立即修复，改用 tokio::fs 或 spawn_blocking | |
| P0-8 | 正确性 | `settings` 路由 DB 错误仍返回 HTTP 200 | `src/routes/settings.rs:47-78` | 高 | confirmed | 立即修复，统一返回正确 HTTP 状态码 | |
| P0-9 | 正确性 | 静态资源 fallback 导致 `/api/404` 返回 `index.html` + 200 | `src/static_assets/mod.rs:9-31` | 高 | skipped | 跳过不改 | |
| P0-10 | 数据一致性 | `cron_jobs.group` / `is_deleted` 未声明默认值，新库与迁移库 schema 不一致 | `src/entity/cron_job.rs:15,21` | 高 | done | 修复：group 默认 other，is_deleted 默认 false | |
| P0-11 | 数据一致性 | `find_by_name` 不区分软删除，已删任务仍可被修改 | `src/cron/repository.rs:98-103` | 高 | done | 立即修复，默认过滤软删除 | |
| P0-12 | 部署安全 | Docker 运行时以 root 用户启动 | `Dockerfile:102-116` | 高 | skipped | 跳过不改 | |

---

## P1 - 短期处理（中风险）

| # | 分类 | 问题摘要 | 位置 | 风险 | 状态 | 用户决策 | 备注 |
|---|------|----------|------|------|------|----------|------|
| P1-1 | 调度器 | Worker `join_handle` 被丢弃，无法优雅关闭 | `src/cron/worker.rs`, `src/lib.rs` | 高 | skipped | 跳过不改 | |
| P1-2 | 调度器 | `JobHandler` 返回 `()`，执行失败无法感知 | `src/cron/mod.rs`, `src/cron/worker.rs` | 高 | confirmed | 改为 Result，worker 记录错误，不重试 | |
| P1-3 | Web | 请求体大小限制 100MB 过大 | `src/routes/mod.rs:19` | 中 | confirmed | 限制为 1MB 或 5MB | |
| P1-4 | 数据库 | Repository 更新先 find 再 update，两次往返 | `src/cron/repository.rs:125-233` | 中 | done | 改为 update_many 单语句 | |
| P1-5 | 数据库 | 路由层多个 repository 操作无事务 | `src/routes/cron_jobs.rs:127-189` | 中 | done | 在 repository 层提供单一事务方法 `update_job_full` | |
| P1-6 | 数据库 | 缺少业务索引 | `src/entity/cron_job.rs:6-22` | 中 | skipped | 跳过不改 | |
| P1-7 | Web | 静态资源缺少 `Cache-Control` / 安全头 | `src/static_assets/mod.rs` | 中 | skipped | 跳过不改 | |
| P1-8 | Web | `healthz` 不检查 DB / scheduler | `src/routes/mod.rs:24-25` | 中 | skipped | 跳过不改 | |
| P1-9 | 前端 | `ky` 无 timeout / retry / 全局错误拦截 | `web/src/lib/api.ts:9-11` | 中 | confirmed | 配置 timeout、retry、beforeError hooks | |
| P1-10 | 前端 | `unwrap` 未统一转换 HTTPError | `web/src/lib/api.ts:13-21` | 中 | confirmed | 统一转换 HTTPError | |
| P1-11 | 前端 | 表单未使用 `react-hook-form` + `zod` | `web/src/pages/cron-jobs.tsx` | 中 | confirmed | 用 zod + react-hook-form 管理表单 | |
| P1-12 | 前端 | 页面过于臃肿，职责不单一 | `web/src/pages/cron-jobs.tsx`, `settings.tsx` | 中 | confirmed | 拆分为 Table/Dialog 等组件 | |
| P1-13 | 前端 | 生产构建无 sourcemap | `web/vite.config.ts:22` | 中 | skipped | 跳过不改 | |
| P1-14 | 前端 | 缺少代码分割 | `web/vite.config.ts:20-23` | 中 | confirmed | 按 vendor 分包 | |
| P1-15 | 前端 | 无路由级懒加载 | `web/src/App.tsx` | 中 | confirmed | 添加 React.lazy + Suspense | |
| P1-16 | 前端 | 无 Error Boundary | `web/src/main.tsx:17-24` | 中 | confirmed | 添加 Error Boundary | |
| P1-17 | 前端 | `index.html` `lang="en"` 与中文界面不符 | `web/index.html:2` | 中 | confirmed | 改为 zh-CN | |
| P1-18 | 前端 | `tsconfig.json` 缺少 `noUncheckedIndexedAccess` | `web/tsconfig.json` | 中 | confirmed | 启用并修复暴露的问题 | |
| P1-19 | DevOps | 缺少 `HEALTHCHECK` | `Dockerfile:102-116` | 中 | confirmed | 添加 HEALTHCHECK | |
| P1-20 | DevOps | `COPY web ./` 会带入 `.env.local` 等文件 | `Dockerfile:16` | 中 | confirmed | 精确 COPY：先 package.json 安装，再 COPY 源码 | |
| P1-21 | DevOps | 前端构建产物未校验 | `Dockerfile:97` | 中 | confirmed | 添加 test -f /app/web/dist/index.html | |
| P1-22 | DevOps | 运行时镜像 `base-ffmpeg` 体积/攻击面不可控 | `Dockerfile:102` | 中 | skipped | 跳过不改 | |
| P1-23 | DevOps | 构建/运行时镜像硬编码内部私有仓库 | `Dockerfile:3-4` | 中 | skipped | 跳过不改 | |
| P1-24 | DevOps | 未创建 `/config/logs` 并设置权限 | `Dockerfile:111` | 中 | confirmed | 创建 db 和 logs 目录并设置权限 | |
| P1-25 | CI | `on: push` 任意分支都会触发并推送 `latest` | `.gitea/workflows/build.yaml:3-4` | 中 | skipped | 跳过不改 | |
| P1-26 | CI | Harbor 用户名硬编码 `admin` | `.gitea/workflows/build.yaml:27-29` | 中 | skipped | 跳过不改 | |
| P1-27 | CI | 缺少镜像安全扫描 | `.gitea/workflows/build.yaml` | 高 | skipped | 跳过不改 | |
| P1-28 | CI | 缺少 `cargo test`、`pnpm lint`、`pnpm test` | `.gitea/workflows/build.yaml` | 中 | skipped | 跳过不改 | |
| P1-29 | CI | 通知 webhook 硬编码内网地址 | `.gitea/workflows/build.yaml:74-77` | 中 | skipped | 跳过不改 | |
| P1-30 | Rust | `SchedulerError` 手动实现，缺失 `source()` 链 | `src/cron/mod.rs:33-72` | 中 | confirmed | 引入 thiserror | |
| P1-31 | Rust | `RuntimeEnv::from_str` 错误类型为 `()` | `src/config/mod.rs:7-16` | 中 | skipped | 跳过不改 | |

---

## P2 - 长期优化（低风险）

| # | 分类 | 问题摘要 | 位置 | 风险 | 状态 | 用户决策 | 备注 |
|---|------|----------|------|------|------|----------|------|
| P2-1 | 调度器 | 调度器状态以 DB 为 source of truth | `src/cron/scheduler.rs:202-403` | 高 | confirmed | 先写 DB 再提交内存 | |
| P2-2 | 数据库 | 时间字段改用 UTC 存储 | `src/entity/cron_job.rs:16-19`, `src/entity/setting.rs:62`, 各调度/Repository 文件 | 低 | done | 改为 UTC 存储 | |
| P2-3 | 数据库 | 增加 `deleted_at` 与任务执行状态字段 | `src/entity/cron_job.rs` | 低 | skipped | 跳过不改 | |
| P2-4 | 数据库 | 连接池大小、`busy_timeout`、`sqlx_logging` 配置化 | `src/db.rs:31,37,50` | 低/中 | skipped | 跳过不改 | |
| P2-5 | Web | 暴露 `/readyz` / `/livez` / `/metrics` | `src/routes/mod.rs` | 低 | skipped | 跳过不改 | |
| P2-6 | 数据库 | 每次启动执行 `ANALYZE` | `src/db.rs:111` | 中 | done | `migrate()` 返回是否发生 schema 变更，按需执行 ANALYZE | |
| P2-7 | 数据库 | 迁移吞掉 `duplicate column name` 错误 | `src/db.rs:142-148` | 中 | done | 用 PRAGMA table_info 前置判断 | |
| P2-8 | 调度器 | Worker 每个任务 spawn 两层 task | `src/cron/worker.rs:56-120` | 中 | confirmed | 简化为单层 spawn | |
| P2-9 | 调度器 | `@every` next run 计算逻辑重复 | `src/cron/worker.rs:63-102` | 中 | confirmed | 提取复用函数 | |
| P2-10 | 调度器 | 启动时大量 missed job 可能同时触发 | `src/cron/scheduler.rs:135-148` | 中 | confirmed | 跳过 missed job，重新计算下次执行时间 | |
| P2-11 | Rust | `tokio` 使用 `features = ["full"]` | `Cargo.toml:18` | 中 | confirmed | 精简 feature | |
| P2-12 | Rust | 直接依赖 `log` crate | `Cargo.toml:12` | 低 | confirmed | 移除 log 依赖 | |
| P2-13 | Rust | `sea-orm` 使用 RC 版本 | `Cargo.toml:15` | 中 | needs-info resolved | 当前最新仍为 rc.41，保持现状 | |
| P2-14 | Rust | 非测试代码中的 `unwrap` | `src/static_assets/mod.rs`, `src/cron/worker.rs:48-52` | 中/高 | confirmed | 替换 unwrap/expect | |
| P2-15 | Rust | 使用 `eprintln!` 而不是 tracing | `src/db.rs:17-21`, `src/logs_cleanup.rs` | 中 | confirmed | 统一使用 tracing | |
| P2-16 | Rust | 测试使用 `unsafe` 修改环境变量 | `src/config/mod.rs:77` | 低 | confirmed | 引入 temp_env | |
| P2-17 | 前端 | `useToast` 模块级全局状态 | `web/src/hooks/use-toast.ts` | 低 | confirmed | 改为 Context/Provider | |
| P2-18 | 前端 | 图标按钮可访问性不足 | `web/src/pages/cron-jobs.tsx` | 低 | confirmed | 增加 sr-only 文本 | |
| P2-19 | 前端 | 查询键未按 name 细分 | `web/src/hooks/use-cron-jobs.ts` | 低 | skipped | 不需要层级查询键 | |
| P2-20 | 前端 | healthz 未用统一 `api` 实例 | `web/src/pages/index.tsx` | 低 | confirmed | 使用 api 实例 | |
| P2-21 | DevOps | 未声明 `VOLUME` | `Dockerfile` | 低 | skipped | 跳过不改 | |
| P2-22 | CI | 缺少镜像 digest 输出 | `.gitea/workflows/build.yaml` | 低 | done | 输出 digest | |
