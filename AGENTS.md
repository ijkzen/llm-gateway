# AGENTS.md

本文件面向 AI 编码助手，用于快速了解 `llm-gateway` 项目的结构、技术栈、构建方式与开发约定。

## 项目概述

- **名称**: `llm-gateway`（Rust crate 名）/ `llm-gateway-web`（前端 npm 包名）
- **版本**: `0.1.7`
- **定位**: 一个 Rust + React 的单体应用模板，提供内嵌管理后台的后端服务。
- **核心功能**:
  - 定时任务（Cron Job）调度管理：列出、启用/禁用、立即执行、编辑、软删除。
  - 定时任务执行日志查看：捕获 handler 内的 tracing 日志，历史保留最近 30 次执行（单次上限 2000 条），SSE 实时推送执行中日志。
  - 系统设置管理：列出、按声明类型校验并修改键值。
  - 登录认证（单用户）：首次启动走初始化流程创建管理员（argon2 哈希），Cookie Session（7 天）保护 `/api/*`；设置页可修改密码（吊销其他会话）。
  - 供应商 API Key 管理：服务端生成 `lg-` 密钥，AES-256-GCM 加密存储（含 SHA-256 `key_hash` 供鉴权查找）。
  - `/v1` OpenAI 兼容转发：对外仅提供 `POST /v1/chat/completions`（Bearer API Key 鉴权），按虚拟模型 LB 策略选成员转发到 OpenAI Compatible / OpenAI Responses / Anthropic / Gemini 四种上游协议（转换逻辑参考 nyro 与 LiteLLM），支持流式与非流式、failover 重试（408/429/500/502/503/529）。
  - 请求指标：每次转发成功/失败各落一行 `request` 表（ttft、tps、缓存命中率等 19 个字段），供后续指标展示。ttft 起点=建连开始（新建连接）或请求发出（复用连接）；tps=output_tokens/(ttft+输出耗时)（流式）或 output_tokens/(end_time−请求发出)（非流式）。
  - 数据面板：`/api/stats/summary`（全量历史累计：请求数/成功率/总 token/加权缓存命中率）+ `/api/stats/charts`（过去 24 小时：按小时分桶趋势 + 按上游 `model_id` 分布）；前端 overview 页（侧边栏「数据面板」）用 Recharts + shadcn chart 展示 4 指标卡与两组三态图表（折线/饼图/降序条形图，Top 10 + 其他）。
  - 供应商用量查询：`GET /api/providers/{id}/usage?refresh=1`（`src/usage/`）。对 `extra.usage=true` 的供应商按 base_url host（火山/阶跃再看 path 区分订阅 Plan 与按量账户）分发到各厂商 fetcher（API key 直查 / Copilot OAuth / 火山与阿里 AK/SK 签名 / CookieCloud cookie 系），归一化为订阅制窗口（5h/周/月，厂商不提供的窗口 `available=false`）或按量余额条目；成功结果写**数据库缓存**（`provider_usage_cache` 表，10 分钟内直出，过期/缺失才真实抓取并重新落库；更新/删除供应商时失效；`?refresh=1` 强制重取），详情页内嵌「用量信息」卡片（进度条按剩余百分比着色）。
  - 用量自动刷新与额度门控：内置定时任务 `usage_refresh`（`@every 5m`，`src/cron/seed.rs` 种子行）刷新全部用量供应商（**不含 enable 过滤**，停用的也持续监测）并落库；订阅制供应商额度耗尽（任一厂商已提供的窗口剩余为 0）时自动停用该 Provider 及名下全部虚拟模型子模型，恢复后自动启用（`src/usage/persist.rs::apply_usage_gate`）。
  - 虚拟模型 LB 用量感知排序：策略 0/1（订阅制优先/按量优先）分组后组内排序——订阅制按 5h→周→月**剩余百分比**逐层比较、三层全平随机选一，按量付费按剩余金额合计降序；用量优先取 10 分钟数据库缓存，缺失/过期才真实抓取（`src/proxy/usage_rank.rs`）。**剔除口径**：订阅制任一已提供窗口剩余为 0（`UsageData::subscription_usable()` 为 false）即从候选剔除；按量付费查得到余额且合计为 0（`UsageData::balance_usable()` 为 false）即从候选剔除，查不到余额的按量成员**不剔除**（无法判定视为可用）；剔除后的顺序即 failover 尝试顺序（降级策略=1 时每次失败在剩余成员中按同策略重新选路）。
- **运行方式**: 后端启动后监听 `0.0.0.0:4007`，前端 SPA 以静态资源形式内嵌在后端中，通过 `/api/*` 与后端通信。

## 技术栈

### 后端

- **语言/运行时**: Rust 2024 edition，Tokio 异步运行时。
- **Web 框架**: Axum 0.8。
- **ORM/数据库**: SeaORM 2.0.0-rc.40 + SQLite（`sqlx-sqlite`、`runtime-tokio-rustls`）。
- **定时调度**: `tokio-cron-scheduler` 触发 + 自研 `src/cron/` 模块（解析、持久化、工作池）；`croner` 解析 Cron 表达式，同时支持 `@every <duration>` 语法。
- **日志捕获与实时推送**: 自定义 `JobLogLayer`（tracing-subscriber Layer）捕获任务 span 内日志，经 std 通道桥接到 `tokio::sync::broadcast`（容量 8192）；SSE 用 axum 自带 `response::sse`（0.8 默认可用，无需 feature）+ `tokio-stream` 的 `BroadcastStream`。
- **日志**: `tracing` + `tracing-subscriber`（JSON 格式输出到 stdout 与按日滚动的日志文件）。
- **认证**: `argon2`（密码哈希）；会话存 `session` 表（只存令牌 SHA-256），HttpOnly Cookie；`src/auth/` 提供 `/api/*` 会话中间件与 `/v1/*` Bearer 中间件。
- **转发上游客户端**: 自研 `src/proxy/upstream.rs` + `src/proxy/pool.rs`（hyper 1 + tokio-rustls + webpki-roots，仅 HTTP/1.1）：连接按 `scheme://host:port` 池化复用（响应体读完归还，空闲超 10 分钟由后台任务释放）；TTFT 起点=建连开始（新建连接）/请求发出（复用连接），建连耗时并入 ttft（无独立 network_latency 字段）；协议转换在 `src/proxy/convert/`。
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
- **图表**: Recharts 2 + shadcn chart 组件（`web/src/components/ui/chart.tsx`），配色走 `--chart-1..5` CSS 变量（亮暗两套，`index.css` 定义）。
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
│   ├── state.rs            # AppState（db + scheduler + lb_state + usage_cache）
│   ├── logs_cleanup.rs     # 日志过期清理
│   ├── response.rs         # 统一 API 响应结构
│   ├── static_assets/mod.rs# rust-embed 内嵌前端 dist
│   ├── middleware/mod.rs   # CORS、Trace、CatchPanic 中间件
│   ├── auth/mod.rs         # 登录认证：argon2 密码哈希、session 表、/api 与 /v1 拦截中间件
│   ├── proxy/              # /v1 转发核心模块
│   │   ├── mod.rs          # 转发管线：虚拟模型路由、LB 选路、failover、request 表落库
│   │   ├── upstream.rs     # 上游 HTTP 客户端（hyper + tokio-rustls，连接池化复用，TTFT 起点=建连开始/请求发出）
│   │   ├── pool.rs         # 上游连接池（按 host 隔离，响应体读完归还，空闲 10 分钟释放）
│   │   ├── convert/        # 协议转换：openai(直通)/responses/anthropic/gemini（请求+响应+流式+usage 归一）
│   │   ├── metrics.rs      # request 表记录与流式指标（ttft/输出耗时）
│   │   ├── sse.rs          # SSE 拆分/写出工具
│   │   └── usage_rank.rs   # 用量感知排序纯比较器（订阅 5h→周→月剩余百分比 / 按量余额合计）
│   ├── usage/              # 供应商用量查询：按 base_url host 分发 fetcher，归一化输出
│   │   ├── types.rs        # UsageData/QuotaWindow（available 标记三窗）/BalanceItem
│   │   ├── persist.rs      # 用量数据库缓存（10 分钟新鲜度）+ 全量刷新 + 订阅额度耗尽自动停用/恢复
│   │   ├── http.rs         # reqwest 封装（15s 超时；LLM_GATEWAY_USAGE_HTTP_OVERRIDE 供测试重定向）
│   │   ├── cookiecloud.rs  # CookieCloud 解密（MD5 材料 + EVP_BytesToKey + AES-256-CBC）
│   │   ├── volcengine_sign.rs # 火山 V4 签名（service=ark，scope 以 /request 结尾）
│   │   └── fetchers/       # 各厂商：api_key/balance/cloud_balance(阿里BSS+火山billing AK/SK)/copilot/volcengine/xiaomi/stepfun/alibaba
│   ├── cron/               # 定时任务核心模块
│   │   ├── mod.rs          # JobContext/JobHandler/JobInfo/SchedulerError 定义
│   │   ├── parser.rs       # 表达式解析、下次运行时间与频率计算（服务器本地时区，部署即东八区）
│   │   ├── repository.rs   # CronJobRepository trait 与 SeaORM 实现
│   │   ├── scheduler.rs    # SchedulerRuntime：任务加载、启停、增删改
│   │   ├── scheduler/tests.rs # scheduler 单元测试（FailingRepo 用宏生成透传委托）
│   │   ├── seed.rs         # 内置定时任务种子行（usage_refresh，@every 5m，启动幂等 upsert）
│   │   ├── worker.rs       # JobWorker：有界队列 + 信号量并发池 + 优雅关闭 + 执行日志落库
│   │   ├── log_capture.rs  # JobLogLayer：捕获任务 span 内 tracing 日志 → broadcast
│   │   ├── log_repository.rs # 执行日志持久化（runs/logs 表、30 次清理、启动恢复）
│   │   └── test_utils.rs   # #[cfg(test)] 单元测试共享辅助（setup_db/sample_job）
│   ├── routes/             # Axum 路由
│   │   ├── mod.rs          # create_app(state)：路由组装 + 登录拦截中间件
│   │   ├── auth.rs         # status/init/login/logout/me/change-password
│   │   ├── cron_jobs.rs    # 任务 CRUD + logs 列表/单次日志/SSE 实时流
│   │   ├── openai_compat.rs# /v1/models 元数据 + /v1/chat/completions 转发入口
│   │   ├── stats.rs        # 数据面板：/api/stats/summary 累计指标 + /api/stats/charts 24h 图表聚合
│   │   └── settings.rs
│   └── entity/             # SeaORM 实体
│       ├── mod.rs
│       ├── cron_job.rs
│       ├── cron_job_run.rs # 一次执行的元信息（status/起止时间/日志统计）
│       ├── cron_job_log.rs # 单条日志（run_id + seq + level + message）
│       ├── user.rs         # 管理后台用户（单用户，argon2 哈希）
│       ├── session.rs      # 登录会话（主键为令牌 SHA-256）
│       ├── request.rs      # /v1 每次转发的指标记录（20 字段）
│       ├── usage_cache.rs  # 供应商用量数据库缓存（provider_usage_cache：provider_id 唯一 + usage_json + fetched_at）
│       └── setting.rs
├── tests/                  # 集成测试
│   ├── common/mod.rs       # 集成测试共享引导（内存库 + worker + scheduler；build_authed_app 自动注入测试凭证）
│   ├── auth_integration.rs # 认证：init/登录/拦截/改密踢会话/登出/Bearer
│   ├── proxy_integration.rs# 转发：四协议转换、include_usage 注入、failover、落库、LB 用量排序
│   ├── cron_jobs_integration.rs
│   ├── cron_job_logs_integration.rs
│   ├── provider_quota_gate_integration.rs # 订阅额度耗尽自动停用/恢复 + usage_refresh 种子任务被调度
│   ├── upstream_pool_integration.rs # 上游连接池：复用 / 空闲释放 / Connection: close 不归还
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
└── .github/workflows/     # GitHub Actions CI（ci.yml 测试 / docker.yml 推 GHCR）
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
5. `runtime`: 基于 `debian:bookworm-slim` 运行二进制（rustls 纯 Rust TLS，无系统 OpenSSL 依赖），暴露 `4007`；安装 `curl` 并配置 `HEALTHCHECK` 检查 `/api/healthz`。

## 测试说明

- **Rust 测试**: `cargo test`。当前共 210 个单元测试（`auth`、`config`、`cron::*`（含 `seed` 种子幂等）、`crypto`、`db`、`logs_cleanup`、`proxy::convert`（四协议转换）、`proxy::sse`、`proxy::upstream`、`proxy::usage_rank`（订阅 5h→周→月比较链/按量余额排序）、`usage::*`（各厂商用量解析/签名/CookieCloud 解密 + `persist` 缓存写读与 10 分钟过期判定 + 额度判定谓词）等模块）+ 集成测试（`tests/auth_integration.rs`、`tests/proxy_integration.rs`（本地 mock 上游）、`tests/cron_jobs_integration.rs`、`tests/cron_job_logs_integration.rs`、`tests/settings_integration.rs`、`tests/providers_integration.rs`、`tests/provider_models_integration.rs`、`tests/api_keys_integration.rs`、`tests/virtual_models_integration.rs`、`tests/virtual_models_openai_integration.rs`、`tests/stats_integration.rs`、`tests/provider_usage_integration.rs`（用量查询：404/未开启/不支持 host + 数据库缓存 10 分钟过期重取 + `refresh_all_usage` 只写用量供应商<含停用>）、`tests/provider_quota_gate_integration.rs`（额度耗尽停用/恢复 + 种子任务被调度）、`tests/upstream_pool_integration.rs`（连接池：同一上游复用连接 / 空闲超时释放 / `Connection: close` 不归还，mock server 手动计数连接数）、`tests/provider_usage_integration.rs` 经 `LLM_GATEWAY_USAGE_HTTP_OVERRIDE` 重定向到本地 mock）。注意：依赖全局 tracing subscriber 的测试（`log_capture` 与 worker 日志链路测试）通过 `SUBSCRIBER_LOCK` 串行执行；worker 日志测试需用 `current_thread` runtime（`set_default` 是线程局部的）。集成测试默认经 `tests/common::build_authed_app` 注入固定凭证（Admin/Password 会话 + `itest-key` Bearer），auth 集成测试用未注入的 `build_app` 验证 401 行为。
- 环境变量隔离使用 `temp-env`，临时目录使用 `tempfile`。
- 调度器测试包含关键行为回归：禁用的任务不会触发（`set_stop` 在 tokio-cron-scheduler 内存存储下无效，禁用必须走移除）、启用后恢复触发、禁用任务仍可手动执行。
- **前端测试**: `cd web && pnpm vitest run`（`pnpm test` 为 watch 模式）。现有 27 个测试文件 109 个用例，位于 `web/src/__tests__/` 与 `web/src/components/__tests__/`（含 login 页、RequireAuth 守卫、ChangePasswordDialog、ProviderUsageCard）。注意：`web/src/test/setup.ts` 中为 Node 26 与 jsdom 的全局 `localStorage` 冲突做了内存 polyfill；`cron-job-logs-dialog` 测试用 MockEventSource 驱动 SSE 事件（`act` 包裹）并 mock 数据 hooks。
- 没有 E2E 测试。

## 代码风格与开发约定

### 提交前置动作（强制门禁）

**任何代码提交前，必须先完成以下校验且全部通过（全绿），否则不允许提交**：

```bash
cargo fmt                          # 后端全库格式化（非局部）
cargo clippy --all-targets --all-features -- -D warnings   # 修复到零警告
cargo test --all-targets           # 后端全量测试
cd web && pnpm lint                # 前端 biome check .（tab 缩进、双引号、100 列）
pnpm vitest run                    # 前端全量测试
```

- 以上为 CI 的实际门禁（`ci.yml`：`cargo fmt --check` + `clippy -D warnings` + `cargo test --all-targets`；前端本地仍须 `pnpm lint` 全绿）。
- **既有代码引发的差异/警告/错误也必须一并修复**，不允许带警告提交，也不要回退 rustfmt/clippy 版本或跳过。
- 发布流程（release-management skill）中的「全量质量门」即本约定，必须全绿。

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

- **Handler 注册**：业务 Handler 在 `src/lib.rs::init()` 中通过 `scheduler.register_handler(name, handler)` 注册，当前注册了 `example` 示例（多步间隔输出日志，用于演示实时日志）与 `usage_refresh`（每 5 分钟刷新全部已开启用量展示的供应商用量并落库、执行订阅额度门控，见 `src/usage/persist.rs`）。内置周期任务在启动时经 `src/cron/seed.rs` 幂等插入种子行（`usage_refresh` / `@every 5m`），无创建任务的 API。Handler 类型：

  ```rust
  pub type JobHandler = Arc<
      dyn Fn(JobContext) -> Pin<Box<dyn Future<Output = Result<(), JobError>> + Send>> + Send + Sync,
  >;
  ```

  `JobContext` 目前只携带 `db: DatabaseConnection`。

- **执行日志**：worker 执行 handler 时创建带 `job_name`/`run_id` 字段的 span，`JobLogLayer` 捕获 span 内的 tracing 日志（级别受 `RUST_LOG` 限制，默认 info 起步）后经 broadcast 分发：worker 落库（`cron_job_runs` + `cron_job_logs`），SSE 端点实时推送。每次执行算一个 run，任务最多保留最近 30 次执行（更早的连同日志清理），单次执行最多 2000 条日志（超出丢弃并标记截断）。handler 失败/panic 会追加一条 `任务执行失败：...` 系统日志；进程重启时残留的 running run 会被标记为 failed。手动「立即执行」与调度触发走同一通道，同样记录日志。
- **日志 API**：`GET /api/cron-jobs/{name}/logs`（最近 30 次执行列表）、`GET /api/cron-jobs/{name}/logs/{run_id}`（某次执行的全部日志，404 若不存在）、`GET /api/cron-jobs/{name}/logs/stream`（SSE：连接时若任务在执行中先发 `snapshot` 回放已落库日志，否则发 `idle`；后续推送 `log`/`run_started`/`run_ended`；接收端积压时发 `reset` 让前端重拉）。

- **禁用 = 从调度器移除**：tokio-cron-scheduler 的 `set_stop()` 在其内存存储实现下并不会阻止任务触发，因此禁用任务时是将 job 从 `JobScheduler` 中移除、但保留在内存列表中（仍可查看与手动“立即执行”）；启用时重新创建 job。
- **时区**：Cron 表达式按**服务器本地时区**解释（`0 0 8 * * *` = 本地时间每天 8 点），与前端浏览器本地时间展示一致。部署容器（根 `Dockerfile` 与 `deploy/Dockerfile`，均安装了 tzdata）设置 `TZ=Asia/Shanghai`，compose 文件亦有 `TZ: Asia/Shanghai` 兜底，因此生产环境 cron 语义为东八区。注意 tokio-cron-scheduler 在创建 job 时快照 UTC 偏移，有夏令时的地区跨切换点时需要重建 job 才会使用新偏移；东八区无夏令时，不受影响。
- **支持的表达式**：
  - 标准 Cron：5 字段（自动补秒为 `0`）或 6 字段（秒 分 时 日 月 周）。
  - 便捷宏：`@yearly`、`@monthly`、`@weekly`、`@daily`、`@hourly`。
  - 间隔语法：`@every 5m`、`@every 1h30m` 等（单位 `s/m/h/d`，可组合）。
- **错过执行**：重启时已过期的 cron 任务不会补跑，`next_run_at` 会被重算到下一次；`@every` 任务重启后从当前时间重新计间隔。
- **优雅关闭**：收到 SIGINT/SIGTERM 后，先停 HTTP 服务，再停调度器（不再派发新任务），最后等待在跑任务结束（10 秒超时后放弃并退出）。
- **API**：只支持更新、立即执行、软删除已有任务，没有创建任务的接口（`SchedulerRuntime::add_job` 可供代码内使用）。对未加载进调度器（无 handler）的任务调用更新接口会直接返回 400，不会修改 DB。

## 部署与 CI/CD

- CI 位于 `.github/workflows/`：
  - `ci.yml`：push 到 main / PR 时运行测试、clippy、fmt（先构建前端）。
  - `nightly.yml`：每次 push 到 main 用 `docker buildx` 构建镜像并推送到 **Nightly 渠道**
    （`ghcr.io/ijkzen/llm-gateway:nightly`，覆盖式），供尝鲜，不建 Release。
  - `release.yml`：**只匹配 `v*` tag**，正式发布渠道。流程：`bash scripts/check-release-version.sh`
    校验 `Cargo.toml` 与 `web/package.json` 版本等于 tag 版本 → `cargo test` + `pnpm vitest run` →
    构建推送 `ghcr.io/ijkzen/llm-gateway:vX.Y.Z`（同时更新 `:latest`）→ `softprops/action-gh-release`
    创建 GitHub Release 页（自动 changelog）。
- **发布版本 ≠ 本地部署**：正式发布（改版本号 + 打 `v*` tag + push，走 release.yml，只涉及 GitHub）
  与 FRP/阿里云本地部署（zig 交叉编译 + `.deploy/deploy.sh`，镜像 tag 固定 `llm-gateway:latest`，
  不读不写版本号）是两条互不相干的流程，发版操作步骤见
  `.agents/skills/release-management/SKILL.md`。
- 生产容器：
  - 暴露端口 `4007`。
  - 需要挂载 `/config/db` 与 `/config/logs` 以保证数据与日志持久化。
  - 运行时镜像（根 `Dockerfile` 的 debian 层与 `deploy/Dockerfile` 的 alpine 层）安装了 tzdata 并设置 `TZ=Asia/Shanghai`（cron 表达式按东八区解释，见调度器说明-时区）；compose 文件的 `TZ` 环境变量作为运行时兜底。`.deploy/deploy.sh` 部署后验证容器时区为 `CST`。

## 安全注意事项

- **认证**: 管理接口（`/api/*`，除 auth/status|login|init 与 healthz）需要 Cookie Session；`/v1/*` 需要 Bearer API Key（api_key 表校验）。密码 argon2id 哈希，session 表只存令牌摘要。SPA 静态资源不做服务端拦截（前端路由守卫负责跳转）。
- **CORS**: `middleware/mod.rs` 中使用了 `CorsLayer::permissive()`，允许任意来源跨域访问 API（包括浏览器中任意网页）。已知风险，部署在不可信网络前务必收敛。Cookie 为 SameSite=Lax，跨站请求不会携带会话。
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

### Release management

正式发布生产/开源/Release 版本（改版本号 → 打 `v*` tag → push → Release CI）的完整流程。
见 `.agents/skills/release-management/SKILL.md`。注意与 FRP 本地部署（`.deploy/deploy.sh`）区分。

### Issue tracker

Issues and specs live as markdown files under `.scratch/<feature-slug>/` in this repo. See `docs/agents/issue-tracker.md`.

### Triage labels

Five canonical roles with default label strings (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: root `CONTEXT.md` + `docs/adr/`. See `docs/agents/domain.md`.
