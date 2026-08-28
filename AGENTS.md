# AGENTS.md

本文件面向 AI 编码助手，用于快速了解 `llm-gateway` 项目的结构、技术栈、构建方式与开发约定。

## 项目概述

- **名称**: `llm-gateway`（Rust crate 名）/ `llm-gateway-web`（前端 npm 包名）
- **版本**: `0.1.0`
- **定位**: 一个 Rust + React 的单体应用模板，提供内嵌管理后台的后端服务。
- **核心功能**:
  - 定时任务（Cron Job）调度管理：列出、启用/禁用、立即执行、编辑、软删除。
  - 定时任务执行日志查看：捕获 handler 内的 tracing 日志，历史保留最近 30 次执行（单次上限 2000 条），SSE 实时推送执行中日志。
  - 系统设置管理：列出、按声明类型校验并修改键值。
- **运行方式**: 后端启动后监听 `0.0.0.0:4007`，前端 SPA 以静态资源形式内嵌在后端中，通过 `/api/*` 与后端通信。

## 技术栈

### 后端

- **语言/运行时**: Rust 2024 edition，Tokio 异步运行时。
- **Web 框架**: Axum 0.8。
- **ORM/数据库**: SeaORM 2.0.0-rc.40 + SQLite（`sqlx-sqlite`、`runtime-tokio-rustls`）。
- **定时调度**: `tokio-cron-scheduler` 触发 + 自研 `src/cron/` 模块（解析、持久化、工作池）；`croner` 解析 Cron 表达式，同时支持 `@every <duration>` 语法。
- **日志捕获与实时推送**: 自定义 `JobLogLayer`（tracing-subscriber Layer）捕获任务 span 内日志，经 std 通道桥接到 `tokio::sync::broadcast`（容量 8192）；SSE 用 axum 自带 `response::sse`（0.8 默认可用，无需 feature）+ `tokio-stream` 的 `BroadcastStream`。
- **日志**: `tracing` + `tracing-subscriber`（JSON 格式输出到 stdout 与按日滚动的日志文件）。
- **静态资源**: `rust-embed` 将 `web/dist` 打包进二进制。
- **配置**: 仅通过环境变量读取，无额外配置框架。

### 前端

- **框架**: React 19 + TypeScript 5.6。
- **构建工具**: Vite 6。
- **路由**: React Router DOM 7。
- **状态/数据获取**: Zustand 5、TanStack Query 5、`ky` HTTP 客户端。
- **表格**: `@tanstack/react-table` 8（列定义、排序、列显隐、分页，封装在 `web/src/components/data-table/`）。
- **UI 组件**: shadcn/ui 风格组件（Radix UI + Tailwind CSS 3.4），使用 `class-variance-authority`、`clsx`、`tailwind-merge`；动画类由 `tailwindcss-animate` 提供（`animate-in`/`fade-in`/`slide-in-from-*` 等）。
- **通知**: `sonner`（`<Toaster />` 挂载在 App 根部，业务代码通过 `web/src/hooks/use-toast.ts` 的 `useToastActions()` 调用 `toastSuccess`/`toastError`）。
- **图标**: `lucide-react`。
- **校验/表单**: Zod、React Hook Form、`@hookform/resolvers`，表单统一使用 `web/src/components/ui/form.tsx` 的 `FormField`/`FormItem`/`FormControl`/`FormMessage` 封装。
- **代码检查/格式化**: Biome 1.9（`web/biome.json` 已配置为 tab 缩进、双引号、100 列最大宽度）。
- **测试**: Vitest 2.1 + `@testing-library/react` + `jsdom`。

## 项目结构

```text
.
├── Cargo.toml              # Rust 项目配置
├── Cargo.lock              # Rust 依赖锁定
├── Dockerfile              # 多阶段构建镜像
├── .env.example            # 环境变量示例
├── src/                    # 后端源码
│   ├── main.rs             # 入口：加载配置并启动服务
│   ├── lib.rs              # 模块导出与 run() 生命周期（含优雅关闭）
│   ├── config/mod.rs       # 环境变量与 Config 结构
│   ├── db.rs               # SeaORM 连接、连接池与自动建表/迁移
│   ├── state.rs            # AppState（db + scheduler）
│   ├── logs_cleanup.rs     # 日志过期清理
│   ├── response.rs         # 统一 API 响应结构
│   ├── static_assets/mod.rs# rust-embed 内嵌前端 dist
│   ├── middleware/mod.rs   # CORS、Trace、CatchPanic 中间件
│   ├── cron/               # 定时任务核心模块
│   │   ├── mod.rs          # JobContext/JobHandler/JobInfo/SchedulerError 定义
│   │   ├── parser.rs       # 表达式解析、下次运行时间与频率计算（本地时区）
│   │   ├── repository.rs   # CronJobRepository trait 与 SeaORM 实现
│   │   ├── scheduler.rs    # SchedulerRuntime：任务加载、启停、增删改
│   │   ├── scheduler/tests.rs # scheduler 单元测试（FailingRepo 用宏生成透传委托）
│   │   ├── worker.rs       # JobWorker：有界队列 + 信号量并发池 + 优雅关闭 + 执行日志落库
│   │   ├── log_capture.rs  # JobLogLayer：捕获任务 span 内 tracing 日志 → broadcast
│   │   ├── log_repository.rs # 执行日志持久化（runs/logs 表、30 次清理、启动恢复）
│   │   └── test_utils.rs   # #[cfg(test)] 单元测试共享辅助（setup_db/sample_job）
│   ├── routes/             # Axum 路由
│   │   ├── mod.rs
│   │   ├── cron_jobs.rs    # 任务 CRUD + logs 列表/单次日志/SSE 实时流
│   │   └── settings.rs
│   └── entity/             # SeaORM 实体
│       ├── mod.rs
│       ├── cron_job.rs
│       ├── cron_job_run.rs # 一次执行的元信息（status/起止时间/日志统计）
│       ├── cron_job_log.rs # 单条日志（run_id + seq + level + message）
│       └── setting.rs
├── tests/                  # 集成测试
│   ├── common/mod.rs       # 集成测试共享引导（内存库 + worker + scheduler）
│   ├── cron_jobs_integration.rs
│   ├── cron_job_logs_integration.rs
│   └── settings_integration.rs
├── web/                    # 前端源码
│   ├── package.json
│   ├── pnpm-lock.yaml
│   ├── vite.config.ts
│   ├── tsconfig.json
│   ├── tailwind.config.ts
│   ├── postcss.config.js
│   ├── components.json     # shadcn/ui 配置
│   ├── index.html
│   └── src/
│       ├── App.tsx
│       ├── main.tsx
│       ├── index.css
│       ├── lib/            # api.ts（ky 封装）、utils.ts、constants.ts
│       ├── hooks/          # use-cron-jobs、use-cron-job-logs（runs/单次日志/SSE 实时流）、use-settings、use-theme（light/dark/system）、use-toast（sonner 封装）等
│       ├── components/     # UI 组件（ui/ 为 shadcn/ui 基础组件，data-table/ 为 react-table 封装，
│       │                   # cron-jobs/ 下含 CronJobLogsDialog 日志弹窗等，另有 confirm-dialog、
│       │                   # skip-to-main、theme-toggle 等共享组件）
│       └── pages/          # overview、cron-jobs、settings、not-found（404）页面
├── db/                     # 开发用 SQLite 数据库目录
├── logs/                   # 开发用日志目录
└── .gitea/workflows/       # Gitea Actions CI 工作流
```

## 配置与环境变量

环境变量通过 `.env.example` 说明，运行时由 `src/config/mod.rs` 读取：

| 变量名 | 说明 | 默认值 |
| --- | --- | --- |
| `BIND_ADDRESS` | 后端监听地址 | `0.0.0.0:4007` |
| `APP_ENV` | 运行环境，`dev` 或 `prod` | `dev` |
| `DATABASE_URL` | SQLite 数据库 URL | `sqlite://db/app.db?mode=rwc`（dev）/ `sqlite:///config/db/app.db?mode=rwc`（prod） |
| `RUST_LOG` | tracing 日志级别 | `info,sqlx::query=warn` |
| `CRON_JOB_QUEUE_SIZE` | 定时任务派发队列容量（必须为正整数） | `1000` |
| `CRON_JOB_MAX_CONCURRENT` | 定时任务最大并发执行数（必须为正整数） | `10` |

注意：

- `APP_ENV=prod` 时，数据库路径与日志路径会切换到 `/config/db` 与 `/config/logs`，便于容器挂载卷。
- 开发时可将 `.env.example` 复制为 `.env` 并加载（Cargo 不会自动读取 `.env`，需自行通过 `dotenv` 等方式加载，当前代码未集成 `dotenv`）。

## 构建与运行

### 后端

```bash
# 调试构建
cargo build

# 运行（需要对应环境变量）
cargo run

# 发布构建
cargo build --release
```

### 前端

```bash
cd web
pnpm install

# 开发服务器，会代理 /api 到 http://localhost:4007
pnpm dev

# 构建产物到 web/dist（后端发布构建会内嵌该目录）
pnpm build

# 代码检查与格式化
pnpm lint
pnpm format

# 运行测试（watch 模式；一次性运行用 pnpm vitest run）
pnpm test
```

### 完整本地启动

1. 启动后端：`cargo run`（确保 `DATABASE_URL` 等环境变量已设置）。
2. 启动前端开发服务器：`cd web && pnpm dev`，访问 Vite 给出的地址，API 请求会自动代理到后端。

### Docker 构建

```bash
docker build -t llm-gateway:latest .
```

Dockerfile 为多阶段构建：

1. `web-builder`: 先精确复制 `package.json`/`pnpm-lock.yaml` 并执行 `pnpm install --frozen-lockfile`，再复制完整 `web` 源码并执行 `pnpm build`。
2. `planner`: `cargo chef prepare` 生成 recipe。
3. `rust-deps`: `cargo chef cook` 预编译 Rust 依赖。
4. `builder`: 复制依赖缓存与前端 dist，校验 `web/dist/index.html` 存在后执行 `cargo build --release`。
5. `runtime`: 从 `192.168.31.100:2080/ijkzen/base-ffmpeg:v0.8` 基础镜像运行二进制，暴露 `4007`；安装 `curl`（优先 `apt-get`，回退 `apk`）并配置 `HEALTHCHECK` 检查 `/api/healthz`。

## 测试说明

- **Rust 测试**: `cargo test`。当前共 96 个测试：74 个单元测试（`config`、`cron::log_capture`、`cron::log_repository`、`cron::parser`、`cron::repository`、`cron::scheduler`、`cron::worker`、`db`、`logs_cleanup` 模块）+ 22 个集成测试（`tests/cron_jobs_integration.rs`、`tests/cron_job_logs_integration.rs`、`tests/settings_integration.rs`）。注意：依赖全局 tracing subscriber 的测试（`log_capture` 与 worker 日志链路测试）通过 `SUBSCRIBER_LOCK` 串行执行；worker 日志测试需用 `current_thread` runtime（`set_default` 是线程局部的）。
- 环境变量隔离使用 `temp-env`，临时目录使用 `tempfile`。
- 调度器测试包含关键行为回归：禁用的任务不会触发（`set_stop` 在 tokio-cron-scheduler 内存存储下无效，禁用必须走移除）、启用后恢复触发、禁用任务仍可手动执行。
- **前端测试**: `cd web && pnpm vitest run`（`pnpm test` 为 watch 模式）。现有 16 个测试文件 42 个用例，位于 `web/src/__tests__/` 与 `web/src/components/__tests__/`。注意：`web/src/test/setup.ts` 中为 Node 26 与 jsdom 的全局 `localStorage` 冲突做了内存 polyfill；`cron-job-logs-dialog` 测试用 MockEventSource 驱动 SSE 事件（`act` 包裹）并 mock 数据 hooks。
- 没有 E2E 测试。

## 代码风格与开发约定

### Rust

- 使用 2024 edition。
- 模块组织：按功能拆分为 `config`、`db`、`entity`、`cron`、`routes`、`middleware` 等模块。
- 统一 API 响应：使用 `src/response.rs` 中的 `Response<T>`，字段名为 `code`、`msg`、`data`，成功时 `code` 为字符串 `"0"`；错误码常量和错误响应 helper（`bad_request`/`not_found` 等）也定义在 `src/response.rs`，路由层统一使用；错误消息使用中文。
- 错误处理：后端大量使用 `anyhow::Result` 与自定义 `SchedulerError`；HTTP 层返回统一 JSON 错误响应。
- 数据库：实体使用 SeaORM 的派生宏；迁移逻辑写在 `src/db.rs` 的 `migrate()` 中，启动时自动建表并通过 `schema_migrations` 版本表做增量迁移。
- 日志：结构化 JSON 日志，开发时同时输出到 stdout 与 `logs/app.YYYY-MM-DD`。

### 前端

- TypeScript 严格模式开启，`noUnusedLocals`、`noUnusedParameters` 开启。
- 路径别名 `@/` 指向 `web/src/`。
- UI 组件按 shadcn/ui 约定放在 `web/src/components/ui/`，通过 `cn()` 合并 Tailwind 类名。
- 页面放在 `web/src/pages/`，通用布局在 `web/src/components/layout.tsx`。
- Hooks 放在 `web/src/hooks/`，API 封装在 `web/src/lib/api.ts`（`ApiResponse<T>` 对应后端 `code/msg/data`，`code !== "0"` 抛 `ApiError`）。
- 使用函数式组件与 Hooks，状态管理以 TanStack Query 为主，Zustand 用于主题等全局状态（`web/src/hooks/use-theme.ts`）。
- UI 文案为中文（如“定时任务”、“设置”、“操作成功”）。

## 数据库与迁移

- 使用 SQLite，默认开启 WAL 模式、`synchronous=NORMAL`、外键、5 秒 busy timeout、约 256 MB 页缓存与 256 MB mmap。
- 启动时 `src/db.rs::migrate()` 自动建表（`cron_jobs`、`setting`、`schema_migrations`），并按版本号执行增量迁移；schema 变化后执行一次 `ANALYZE`。
- `ensure_sqlite_dir` 会从 `DATABASE_URL` 解析出文件路径（保留绝对路径）并预先创建父目录。
- 生产环境数据库文件位于 `/config/db/app.db`，建议挂载持久化卷。

## 调度器说明

定时任务由三层协作完成：

- **SchedulerRuntime**（`src/cron/scheduler.rs`）：管理任务生命周期。启动时从 DB 加载 `is_deleted = false` 的任务，但**要求先在代码中注册对应的 Handler**，否则任务会被跳过（跳过即不出现在列表 API 中，也无法通过 API 操作）。
- **JobWorker**（`src/cron/worker.rs`）：有界 mpsc 队列 + 信号量并发池，负责真正执行 Handler、记录执行日志并回写 `last_run_at`/`next_run_at`。
- **croner**（`src/cron/parser.rs`）：解析表达式、计算下次运行时间与执行频率。

关键行为约定：

- **Handler 注册**：业务 Handler 在 `src/lib.rs::init()` 中通过 `scheduler.register_handler(name, handler)` 注册，当前注册了一个 `example` 示例（多步间隔输出日志，用于演示实时日志）。Handler 类型：

  ```rust
  pub type JobHandler = Arc<
      dyn Fn(JobContext) -> Pin<Box<dyn Future<Output = Result<(), JobError>> + Send>> + Send + Sync,
  >;
  ```

  `JobContext` 目前只携带 `db: DatabaseConnection`。

- **执行日志**：worker 执行 handler 时创建带 `job_name`/`run_id` 字段的 span，`JobLogLayer` 捕获 span 内的 tracing 日志（级别受 `RUST_LOG` 限制，默认 info 起步）后经 broadcast 分发：worker 落库（`cron_job_runs` + `cron_job_logs`），SSE 端点实时推送。每次执行算一个 run，任务最多保留最近 30 次执行（更早的连同日志清理），单次执行最多 2000 条日志（超出丢弃并标记截断）。handler 失败/panic 会追加一条 `任务执行失败：...` 系统日志；进程重启时残留的 running run 会被标记为 failed。手动「立即执行」与调度触发走同一通道，同样记录日志。
- **日志 API**：`GET /api/cron-jobs/{name}/logs`（最近 30 次执行列表）、`GET /api/cron-jobs/{name}/logs/{run_id}`（某次执行的全部日志，404 若不存在）、`GET /api/cron-jobs/{name}/logs/stream`（SSE：连接时若任务在执行中先发 `snapshot` 回放已落库日志，否则发 `idle`；后续推送 `log`/`run_started`/`run_ended`；接收端积压时发 `reset` 让前端重拉）。

- **禁用 = 从调度器移除**：tokio-cron-scheduler 的 `set_stop()` 在其内存存储实现下并不会阻止任务触发，因此禁用任务时是将 job 从 `JobScheduler` 中移除、但保留在内存列表中（仍可查看与手动“立即执行”）；启用时重新创建 job。
- **时区**：Cron 表达式按**服务器本地时区**解释（`0 0 8 * * *` = 本地时间每天 8 点），与前端本地时间展示一致。注意 tokio-cron-scheduler 在创建 job 时快照 UTC 偏移，有夏令时的地区跨切换点时需要重建 job 才会使用新偏移。
- **支持的表达式**：
  - 标准 Cron：5 字段（自动补秒为 `0`）或 6 字段（秒 分 时 日 月 周）。
  - 便捷宏：`@yearly`、`@monthly`、`@weekly`、`@daily`、`@hourly`。
  - 间隔语法：`@every 5m`、`@every 1h30m` 等（单位 `s/m/h/d`，可组合）。
- **错过执行**：重启时已过期的 cron 任务不会补跑，`next_run_at` 会被重算到下一次；`@every` 任务重启后从当前时间重新计间隔。
- **优雅关闭**：收到 SIGINT/SIGTERM 后，先停 HTTP 服务，再停调度器（不再派发新任务），最后等待在跑任务结束（10 秒超时后放弃并退出）。
- **API**：只支持更新、立即执行、软删除已有任务，没有创建任务的接口（`SchedulerRuntime::add_job` 可供代码内使用）。对未加载进调度器（无 handler）的任务调用更新接口会直接返回 400，不会修改 DB。

## 部署与 CI/CD

- CI 位于 `.gitea/workflows/build.yaml`，在每次 `push` 时触发。
- 使用 Gitea Actions 在 `ubuntu-latest` 上执行：
  1. 检出代码。
  2. 登录内部 Harbor 镜像仓库 `192.168.31.100:2080`。
  3. 使用 `docker buildx` 构建 `linux/amd64` 镜像，推送到 `192.168.31.100:2080/ijkzen/llm-gateway:latest`。
- 生产容器：
  - 暴露端口 `4007`。
  - 需要挂载 `/config/db` 与 `/config/logs` 以保证数据与日志持久化。

## 安全注意事项

- **无身份验证**: 当前后端没有登录、权限或 API Token 机制，默认对外开放所有接口，仅限可信局域网使用。
- **CORS**: `middleware/mod.rs` 中使用了 `CorsLayer::permissive()`，允许任意来源跨域访问 API（包括浏览器中任意网页）。已知风险，部署在不可信网络前务必收敛。
- **Panic 处理**: 通过 `CatchPanicLayer` 捕获 panic，返回 HTTP 500 与中文提示“服务器内部错误”。
- **敏感信息**: Dockerfile 与 CI 工作流中硬编码了内部镜像仓库地址、S3 端点与访问密钥，生产部署前应改为 Secret/环境变量注入。
- **文件权限**: 生产数据库目录 `/config/db` 与日志目录 `/config/logs` 在 Dockerfile 中创建并设置 `755`。
- **输入校验**: HTTP 层做基础校验（Cron 表达式可解析、设置值按声明类型校验），关键业务逻辑由调度器内部再次校验。

## 常见问题与注意事项

1. **前端构建产物必须存在**: 发布构建时，`rust-embed` 会内嵌 `web/dist`。如果本地手动构建后端，请先执行 `cd web && pnpm build`；Dockerfile 中已自动处理。
2. **定时任务需要注册 Handler 才会执行**: 数据库中的任务若没有对应注册的 Handler，加载时会被跳过，且不会出现在任务列表中；实现业务功能时请先在 `scheduler` 上调用 `register_handler`。
3. **环境变量不会自动加载 `.env`**: 当前未集成 `dotenv`，运行前请确保环境变量已导出。
4. **日志清理**: 后端启动后会启动一个后台任务，每天清理一次日志目录中超过 30 天的文件（按修改时间判断，不区分文件类型，不要往日志目录放其他文件）。
5. **Biome 配置**: `web/biome.json` 已存在，`pnpm lint` 与 `pnpm format` 使用该配置（tab 缩进、双引号、100 列最大宽度）。
6. **健康检查**: `/api/healthz` 只表示进程存活，不检查数据库等依赖。

## Agent skills

### Issue tracker

Issues and specs live as markdown files under `.scratch/<feature-slug>/` in this repo. See `docs/agents/issue-tracker.md`.

### Triage labels

Five canonical roles with default label strings (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: root `CONTEXT.md` + `docs/adr/`. See `docs/agents/domain.md`.
