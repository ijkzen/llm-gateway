# 构建、运行与测试

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
| `API_KEY_ENCRYPTION_KEY` | 敏感字段（如 provider api_key）加密密钥；为空时明文存储，仅限开发 | 空 |

注意：

- `APP_ENV=prod` 时，数据库路径与日志路径切换到 `/config/db` 与 `/config/logs`，便于容器挂载卷。
- 项目未集成 `dotenv`，环境变量不会自动加载。开发时可将 `.env.example` 复制为 `.env` 并自行导出。

## 后端

```bash
cargo build          # 调试构建
cargo run            # 运行（需要对应环境变量）
cargo build --release
```

## 前端

```bash
cd web
pnpm install

pnpm dev      # 开发服务器，代理 /api 到 http://localhost:4007
pnpm build    # 产物到 web/dist（后端发布构建会内嵌该目录）
pnpm lint     # biome check .
pnpm format
pnpm test     # watch 模式；一次性运行用 pnpm vitest run
```

## 完整本地启动

1. 启动后端：`cargo run`（确保 `DATABASE_URL` 等环境变量已设置）。
2. 启动前端开发服务器：`cd web && pnpm dev`，访问 Vite 给出的地址，API 请求自动代理到后端。

## Docker 构建

```bash
docker build -t llm-gateway:latest .
```

多阶段构建：

1. `web-builder`：先精确复制 `package.json`/`pnpm-lock.yaml` 并 `pnpm install --frozen-lockfile`，再复制完整 `web` 源码并 `pnpm build`。
2. `planner`：`cargo chef prepare` 生成 recipe。
3. `rust-deps`：`cargo chef cook` 预编译 Rust 依赖。
4. `builder`：复制依赖缓存与前端 dist，校验 `web/dist/index.html` 存在后 `cargo build --release`。
5. `runtime`：基于 `debian:bookworm-slim` 运行二进制（rustls 纯 Rust TLS，无系统 OpenSSL 依赖），暴露 `4007`；安装 `curl` 并配置 `HEALTHCHECK` 检查 `/api/healthz`。

## 数据库与迁移

- 使用 SQLite，默认开启 WAL 模式、`synchronous=NORMAL`、外键、5 秒 busy timeout、约 256 MB 页缓存与 256 MB mmap。
- 启动时 `src/db.rs::migrate()` 自动建表（`cron_jobs`、`setting`、`schema_migrations`），并按版本号执行增量迁移。
- **当前编至 29**：迁移 26 建 `request_log_snapshot`/`snapshot_meta` 与复合唯一索引（ADR-0021），27 建 `notification_channel`（ADR-0023），28 将 `virtual_model_item.model_id` 唯一索引换为普通索引（ADR-0026），29 建 `speed_test_run`/`speed_test_result`（ADR-0027）。
- **新迁移编号从 16 起**：生产库 14/15 号段被旧 lg-proxy 方案废弃占用。
- schema 变化后执行一次 `ANALYZE`。
- `ensure_sqlite_dir` 从 `DATABASE_URL` 解析文件路径（保留绝对路径）并预先创建父目录。
- 生产环境数据库文件位于 `/config/db/app.db`，建议挂载持久化卷。

## 测试

### Rust

`cargo test` / `cargo test --all-targets`。当前 1068 个测试函数：`src/` 内单元测试 637 个，`tests/` 集成测试 431 个（36 个顶层测试文件）。

单元测试覆盖的重点模块：`auth`、`config`、`app_settings`、`availability`（可用性状态机）、`backup`（JSON 解析/校验）、`cron::*`（含 `seed` 幂等）、`crypto`、`db`（迁移建表）、`provider_model`（含 `catalog` 外置优先/mtime 缓存/回退内嵌与 `catalog_refresh` 抓取校验原子写）/`provider_repo`/`provider_template`、`proxy::convert`（四协议转换）、`proxy` 头处理、`proxy::metrics`（写者队列：持续低流量下队首行不被无限推迟）、`proxy::sse`、`proxy::usage_rank`（订阅 FEFO 比较链/按量余额排序）、`routes::stats`（桶归并/分位/时区）、`routes::speed_test`（CSV id 解析）、`stats_snapshot`（桶帧/闭桶判定/窗口分解/历法周期、生成器含分位标量与哨兵行、主体键解析与降级守卫、生命周期任务、summary_charts 兑底合并）、`notification::*`（飞书消息体组装与错误码映射、注册轮询响应解析/退避/字段名兼容、通知文案渲染）、`speedtest::*`（槽位单实例与取消旗标、目标 token 夹取、`candidate` 关键词三字段与过滤空判定、`bench` 失败原因按字符截断）、`usage::*`（各厂商用量解析/签名/CookieCloud 解密 + `persist` 缓存写读与 10 分钟过期判定 + 额度判定谓词，含 `has_low_remaining_window` 边界判定 + 混合计费（窗口+钱包）可用性兜底与边界探活跳过）。

集成测试分布在 `tests/`，其中六个曾超 1000 行的大文件（proxy_integration / provider_models_integration / provider_usage_integration / stats_integration / virtual_models_integration / notification_integration）已按域拆分为同目录子模块（`tests/<stem>/`，由根文件 `#[path]` 引入，helpers 留在根文件），target 名与测试函数数不变。

集成测试里几个容易漏的定位点，改对应代码时优先看它们：

| 域 | 测试 |
| --- | --- |
| 上游中断回归 | `tests/proxy_integration/` 下的 `upstream_abort` |
| Responses 转换流式 live 转发 | `tests/proxy_integration/` 下的 `responses_live` 子模块 |
| 弹窗测速端点 | `tests/provider_models_test_integration.rs` |
| 用量全量刷新只写用量供应商（含停用） | `tests/provider_usage_integration/` 下的 `refresh_all_usage` |

关键约定：

- 集成测试为黑盒：真实 app（临时目录 SQLite、随机端口）+ 本地 mock 上游，不需要真实 LLM Key。
- 默认经 `tests/common::build_authed_app` 注入固定凭证（`Admin`/`Password` 会话 + `itest-key` Bearer）；auth 集成测试用未注入的 `build_app` 验证 401。
- 环境变量隔离用 `temp-env`，临时目录用 `tempfile`。
- 用量 fetcher 的 HTTP 经 `LLM_GATEWAY_USAGE_HTTP_OVERRIDE` 重定向到本地 mock。
- 依赖全局 tracing subscriber 的测试见 `cron.md`。
- `tests/stats_snapshot_integration.rs` 断言快照路径与实时路径逐字节一致——统计相关改动后它是第一道防线。

### 前端

`cd web && pnpm vitest run`（`pnpm test` 为 watch 模式）。当前 69 个测试文件 554 个用例，分布于 `web/src/__tests__/`（页面级 21 个）、`web/src/components/__tests__/`（组件级 37 个）、各 race 组件、`hooks/__tests__/`、`lib/__tests__/`、`i18n/__tests__/`（键集合一致性）。

`web/src/test/setup.ts` 为 Node 26 与 jsdom 的全局 `localStorage` 冲突做了内存 polyfill，并为 `ResizeObserver`/`scrollIntoView`/`matchMedia` 补了 jsdom 缺失的桩；`cron-job-logs-dialog` 测试用 MockEventSource 驱动 SSE 事件（`act` 包裹）并 mock 数据 hooks。

### 没有 E2E 测试。
