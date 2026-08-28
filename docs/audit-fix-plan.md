# 最佳实践审计整改实施计划

本计划基于 `audit-fix-decisions.md` 中用户确认需要修复的问题，将整改项分组为可独立执行的子任务，后续由子代理逐项实现。

## 实施原则

- 每个任务完成后必须通过 `cargo test` / `cargo clippy` / `pnpm lint`（如适用）。
- 修改遵循最小侵入原则，不改动用户确认跳过的功能。
- 保持现有 API 行为兼容，除非整改本身要求改变行为（如 settings 路由状态码）。

---

## 任务列表

### 任务 1：消除异步上下文中的阻塞 I/O

**涉及问题**：P0-5、P0-6、P0-7、P2-15

**目标**：将 `src/db.rs`、`src/lib.rs`、`src/logs_cleanup.rs` 中的 `std::fs` 阻塞调用改为异步等价物或 `spawn_blocking`；将 `eprintln!` 改为 `tracing`。

**具体修改**：
1. `src/db.rs:7-24`：`ensure_sqlite_dir` 改为 async，使用 `tokio::fs::create_dir_all`。
2. `src/db.rs:17-21`：目录创建失败时使用 `tracing::error!`。
3. `src/lib.rs:37`：`setup_logging` 改为 async 或返回 `Result`，使用 `tokio::fs::create_dir_all`。
4. `src/logs_cleanup.rs:5-25`：`cleanup_old_logs` 改为 async 或包进 `spawn_blocking`；`spawn_cleanup_task` 中的调用改为 await；`eprintln!` 改为 `tracing::error!` / `tracing::warn!`。

**验收标准**：
- `cargo test` 通过。
- `cargo clippy` 无警告。
- 启动流程正常，日志目录/数据库目录创建正常。

---

### 任务 2：SeaORM 实体与 Repository 优化

**涉及问题**：P0-10、P0-11、P1-4、P1-5、P2-6、P2-7

**目标**：修复 schema 默认值、区分软删除、减少 DB 往返、保证 update_job 原子性、优化迁移逻辑。

**具体修改**：
1. `src/entity/cron_job.rs`：给 `group` 加 `#[sea_orm(default_value = "other")]`，给 `is_deleted` 加 `#[sea_orm(default_value = "0")]`。
2. `src/cron/repository.rs`：
   - 默认 `find_by_name` 过滤 `is_deleted = false`；如内部需要查找已删除任务，新增 `find_by_name_including_deleted`。
   - `update_by_name`、`set_enabled`、`soft_delete`、`restore`、`update_run_times` 改为 `Entity::update_many().filter(...).set(...).exec()` 单语句。
   - 新增 `update_job_full_in_txn` 事务方法，将 `enabled`/`info`/`expression` 的更新合并为一个事务。
3. `src/db.rs:111`：`ANALYZE` 改为仅在 schema 变更后执行，或改为后台任务。
4. `src/db.rs:142-148`：用 `PRAGMA table_info(cron_jobs)` 前置判断列是否存在，而不是吞掉 `duplicate column name`。

**验收标准**：
- `cargo test` 通过（包括 repository 单元测试）。
- 新库创建时 schema 与迁移后旧库一致。
- 已软删除任务无法通过 `update_job` 修改。

---

### 任务 3：调度器与 Worker 重构

**涉及问题**：P1-2、P2-1、P2-8、P2-9、P2-10

**目标**：让任务执行错误可记录、以 DB 为 source of truth、简化 Worker task 结构、复用 next run 计算、跳过历史 missed job。

**具体修改**：
1. `src/cron/mod.rs`：将 `JobHandler` 返回类型改为 `Result<(), Box<dyn std::error::Error + Send + Sync>>`。
2. `src/cron/worker.rs`：
   - 根据 handler 返回的 Result 记录错误日志（不重试）。
   - 简化双层 spawn 为单层，使用 `catch_unwind` 捕获 panic。
   - 复用 `parser.rs` 中抽取的 `compute_next_run_from_scheduled_at`。
3. `src/cron/scheduler.rs`：
   - 将 `update_expression`、`update_info`、`soft_delete_job`、`set_enabled` 改为先写 DB，再更新内存调度器。
   - `load_from_db` 中跳过 missed job，直接重新计算 `next_run_at`。

**验收标准**：
- `cargo test` 通过。
- handler 失败时日志中有明确记录。
- 调度器与 DB 状态在崩溃后以 DB 为准。

---

### 任务 4：错误处理与依赖精简

**涉及问题**：P1-30、P2-11、P2-12、P2-14、P2-16

**目标**：用 `thiserror` 简化 `SchedulerError`、精简 tokio feature、移除 `log` 依赖、替换非测试 unwrap、测试用 `temp_env`。

**具体修改**：
1. `Cargo.toml`：
   - 引入 `thiserror`。
   - 将 `tokio` feature 从 `full` 精简为 `rt-multi-thread`、`macros`、`signal`、`sync`、`time`、`fs`。
   - 移除 `log` 依赖。
2. `src/cron/mod.rs`：用 `thiserror` 重写 `SchedulerError`，保留 source 链。
3. `src/static_assets/mod.rs`：替换 `Response::builder().body(...).unwrap()` 为安全错误处理。
4. `src/cron/worker.rs:48-52`：替换 `semaphore.acquire_owned().await.expect(...)` 为错误处理。
5. `src/config/mod.rs:77`：改用 `temp_env` 管理测试环境变量。

**验收标准**：
- `cargo test` 通过。
- `cargo clippy` 无警告。
- 编译后二进制体积不显著增加。

---

### 任务 5：Web 路由与状态码修复

**涉及问题**：P0-8、P1-3

**目标**：统一 settings 路由错误状态码；限制请求体大小。

**具体修改**：
1. `src/routes/settings.rs`：
   - `list_settings` DB 错误返回 `(StatusCode::INTERNAL_SERVER_ERROR, ...)`。
   - `update_setting` 记录不存在返回 404，DB 错误返回 500。
2. `src/routes/mod.rs:19`：将 `DefaultBodyLimit::max(100 * 1024 * 1024)` 改为 `DefaultBodyLimit::max(5 * 1024 * 1024)`（5MB）。

**验收标准**：
- 集成测试覆盖 settings 路由的 404 和 500 状态码。
- `cargo test` 通过。

---

### 任务 6：前端 API 层增强

**涉及问题**：P1-9、P1-10、P2-20

**目标**：统一 `ky` 超时/重试/错误处理，统一转换 HTTPError，healthz 使用 `api` 实例。

**具体修改**：
1. `web/src/lib/api.ts`：
   - `ky.create` 增加 `timeout: 10000`、`retry: 1`。
   - 增加 `beforeError` hook，读取响应 body 的 `msg` 并覆盖 error.message。
   - 导出 `ApiError` 类型。
2. `web/src/pages/index.tsx`（如存在）：healthz 改为使用 `api.get('healthz')`。

**验收标准**：
- `pnpm lint` 通过。
- 后端 500 时前端 Toast 显示中文业务错误信息。

---

### 任务 7：前端表单与组件重构

**涉及问题**：P1-11、P1-12、P2-18

**目标**：cron-jobs 和 settings 页面使用 `react-hook-form` + `zod`；拆分臃肿页面；增加图标按钮 sr-only 文本。

**具体修改**：
1. `web/src/pages/cron-jobs.tsx`：
   - 抽取 `CronJobTable`、`CronJobEditDialog`、`CronJobDeleteDialog` 组件。
   - 编辑表单使用 `zod` schema + `react-hook-form`。
   - 操作按钮增加 `sr-only` 文本。
2. `web/src/pages/settings.tsx`：
   - 抽取 `SettingsTable`、`SettingEditDialog`。
   - 编辑表单使用 `zod` + `react-hook-form`。

**验收标准**：
- `pnpm lint` 通过。
- 表单校验行为与现有逻辑一致（标题/表达式非空）。

---

### 任务 8：前端构建配置与可访问性

**涉及问题**：P1-14、P1-15、P1-16、P1-17、P1-18

**目标**：代码分割、路由懒加载、Error Boundary、HTML lang、启用 `noUncheckedIndexedAccess`。

**具体修改**：
1. `web/vite.config.ts`：增加 `rollupOptions.output.manualChunks`。
2. `web/src/App.tsx`：页面使用 `React.lazy` + `Suspense`。
3. `web/src/main.tsx`：增加 Error Boundary 包裹 `App`。
4. `web/index.html`：`<html lang="zh-CN">`。
5. `web/tsconfig.json`：启用 `noUncheckedIndexedAccess: true`，修复暴露的类型错误。

**验收标准**：
- `pnpm build` 成功。
- `pnpm lint` 通过。
- 首屏加载 JS 体积减少。

---

### 任务 9：Toast 状态管理重构

**涉及问题**：P2-17

**目标**：将 `useToast` 从模块级全局状态改为 Context/Provider。

**具体修改**：
1. `web/src/hooks/use-toast.ts`：
   - 创建 `ToastProvider` 和 `useToast` context hook。
   - 保持现有 API（`toast`、`dismiss`）不变。
2. `web/src/main.tsx`：用 `ToastProvider` 包裹应用。
3. `web/src/components/ui/toaster.tsx`：从 context 读取 toasts。

**验收标准**：
- `pnpm lint` 通过。
- Toast 功能正常，StrictMode 下无异常。

---

### 任务 10：Dockerfile 改进

**涉及问题**：P1-19、P1-20、P1-21、P1-24

**目标**：添加 HEALTHCHECK、精确 COPY web 目录、校验前端产物、创建 logs 目录。

**具体修改**：
1. `Dockerfile` web-builder 阶段：
   - 先 COPY `package.json` + `pnpm-lock.yaml`，执行 `pnpm install --frozen-lockfile`。
   - 再 COPY web 源码，执行 `pnpm build`。
2. `Dockerfile` builder 阶段：增加 `RUN test -f /app/web/dist/index.html`。
3. `Dockerfile` runtime 阶段：
   - 创建 `/config/db` 和 `/config/logs` 并设置权限。
   - 添加 `HEALTHCHECK CMD curl -f http://localhost:4007/api/healthz || exit 1`。

**验收标准**：
- `docker build` 成功。
- 构建失败时（前端产物缺失）明确报错。

---

### 任务 11：CI 输出镜像 digest

**涉及问题**：P2-22

**目标**：CI 推送镜像后输出正确的 registry manifest digest。

**具体修改**：
1. `.gitea/workflows/build.yaml`：
   - `docker buildx build` 改为 `--metadata-file build-metadata.json`。
   - 构建后从 `containerimage.digest` 提取 digest 并输出 `Image digest: sha256:...`。
   - 为 `Build and push image` 步骤设置 `id: build-push`，将 digest 写入 `GITEA_OUTPUT` / `GITHUB_OUTPUT`。
   - 使用 `trap` 清理临时文件，并对 digest 做空/null 校验。

**验收标准**：
- CI workflow YAML 语法正确。
- 构建日志中可见可拉取的 registry manifest digest。

---

### 任务 12：时间字段改用 UTC 存储

**涉及问题**：P2-2

**目标**：将实体与调度相关时间字段从本地时区改为 UTC 存储。

**具体修改**：
1. `src/entity/cron_job.rs`：`last_run_at`、`next_run_at`、`created_at`、`updated_at` 改为 `DateTimeUtc`。
2. `src/entity/setting.rs`：`updated_at` 改为 `DateTimeUtc`。
3. `src/cron/mod.rs`、`src/cron/parser.rs`、`src/cron/repository.rs`、`src/cron/scheduler.rs`、`src/cron/worker.rs`、`src/routes/settings.rs`：所有 `chrono::Local::now()` 改为 `chrono::Utc::now()`，类型改为 `DateTime<Utc>`。

**验收标准**：
- `cargo test` 通过。
- `cargo clippy` 无警告。
- `grep -R "DateTimeLocal\|chrono::Local" src/` 无命中。

---

## 执行顺序

建议按以下顺序执行，减少冲突：

1. 任务 4（错误处理与依赖精简）- 影响 Cargo.toml 和基础类型。
2. 任务 1（阻塞 I/O）- 影响启动流程。
3. 任务 2（SeaORM 实体与 Repository）- 影响数据库层。
4. 任务 3（调度器与 Worker）- 依赖任务 2 的 repository 变更。
5. 任务 5（Web 路由与状态码）- 依赖任务 2/3 的 repository/scheduler。
6. 任务 10（Dockerfile）- 独立。
7. 任务 11（CI）- 独立。
8. 任务 6、7、8、9（前端）- 可并行，但任务 9 在任务 8 之后更稳妥。

---

## 状态跟踪

| 任务 | 状态 | 备注 |
|------|------|------|
| 任务 1：消除异步上下文中的阻塞 I/O | done | 改用 `tokio::fs` / `spawn_blocking`；`eprintln!` 改 tracing |
| 任务 2：SeaORM 实体与 Repository 优化 | done | 默认值、软删除过滤、`update_many`、事务更新、迁移前置判断 |
| 任务 3：调度器与 Worker 重构 | done | `JobHandler` 返回 `Result`、单层 spawn、DB-first、跳过 missed job |
| 任务 4：错误处理与依赖精简 | done | `thiserror`、精简 `tokio` feature、移除 `log`、替换 unwrap、`temp_env` |
| 任务 5：Web 路由与状态码修复 | done | settings 错误状态码、请求体限制 5MB |
| 任务 6：前端 API 层增强 | done | `ky` timeout/retry、`ApiError`、统一错误处理 |
| 任务 7：前端表单与组件重构 | done | 页面拆分、`react-hook-form` + `zod`、可访问性 |
| 任务 8：前端构建配置与可访问性 | done | `manualChunks`、路由懒加载、Error Boundary、`lang=zh-CN`、`noUncheckedIndexedAccess` |
| 任务 9：Toast 状态管理重构 | done | Context/Provider 替代模块级全局状态 |
| 任务 10：Dockerfile 改进 | done | 精确 COPY、产物校验、`/config/logs`、HEALTHCHECK |
| 任务 11：CI 输出镜像 digest | done | `--metadata-file` 提取 `containerimage.digest`、输出 step output、YAML 校验通过 |
| 任务 12：时间字段改用 UTC 存储 | done | 实体与调度时间字段统一 `DateTimeUtc`、`chrono::Utc::now()` |
