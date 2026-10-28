# AGENTS.md

本文件面向 AI 编码助手，用于快速了解 `llm-gateway` 的技术栈、构建方式与开发约定。域机制与运维细节按分支外置在 `docs/agents/`，见「按分支查阅的文档」。

## 项目概述

- **名称**: `llm-gateway`（Rust crate 名）/ `llm-gateway-web`（前端 npm 包名）
- **版本**: `0.1.21`
- **定位**: 一个 Rust + React 的单体应用，提供内嵌管理后台的 LLM 网关服务。
- **运行方式**: 后端监听 `0.0.0.0:4007`，前端 SPA 以静态资源形式内嵌在后端中，通过 `/api/*` 与后端通信；对外转发面为 `/v1/*`。
- **能力面**: 定时任务调度与执行日志、系统设置、单用户登录认证、供应商/供应商模型/虚拟模型管理、`/v1` 四协议转发（chat/completions + messages + responses）与 failover、请求指标与数据面板、供应商用量查询与额度门控、飞书自动通知、模型测速页。

## 技术栈

### 后端

- **语言/运行时**: Rust 2024 edition，Tokio 异步运行时。
- **Web 框架**: Axum 0.8。
- **ORM/数据库**: SeaORM 2.0.0-rc.40 + SQLite（`sqlx-sqlite`、`runtime-tokio-rustls`）。
- **定时调度**: `tokio-cron-scheduler` 触发 + 自研 `src/cron/` 模块（解析、持久化、工作池）；`croner` 解析 Cron 表达式，同时支持 `@every <duration>` 语法。
- **日志捕获与实时推送**: 自定义 `JobLogLayer`（tracing-subscriber Layer）捕获任务 span 内日志，经 std 通道桥接到 `tokio::sync::broadcast`（容量 8192）；SSE 用 axum 自带 `response::sse`（0.8 默认可用，无需 feature）+ `tokio-stream` 的 `BroadcastStream`。
- **日志**: `tracing` + `tracing-subscriber`（JSON 格式输出到 stdout 与按日滚动的日志文件）。
- **认证**: `argon2`（密码哈希）；会话存 `session` 表（只存令牌 SHA-256），HttpOnly Cookie；`src/auth/` 提供 `/api/*` 会话中间件与 `/v1/*` Bearer 中间件。
- **转发上游客户端**: 自研 `src/proxy/upstream.rs` + `src/proxy/pool.rs`（hyper 1 + tokio-rustls + webpki-roots，仅 HTTP/1.1）；协议转换在 `src/proxy/convert/`。
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

## 按分支查阅的文档

改动涉及下列任一分支时，先读对应文档再动手：

| 你要做的事 | 读这个 |
| --- | --- |
| 找文件、看模块边界与整体形状 | `docs/agents/architecture.md`（含完整目录树） |
| 改 `/v1` 转发、协议转换、failover、出站头、思考块透传、LB 排序 | `docs/agents/gateway.md` |
| 改供应商/虚拟模型、用量查询、额度门控、飞书通知、测速页 | `docs/agents/providers.md` |
| 改数据面板、统计端点、统计快照 | `docs/agents/stats.md` |
| 改定时任务、Handler、执行日志、SSE | `docs/agents/cron.md` |
| 改环境变量、构建、迁移、加/改测试 | `docs/agents/build-and-test.md` |
| 改 CI、发布、容器、安全面 | `docs/agents/deploy.md` |
| 领域术语、语言约定 | `CONTEXT.md` |
| 架构决策的原因与取舍 | `docs/adr/`（相关编号） |

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
- **ADR/域文档与代码现状对齐**：提交前查看本次改动涉及的决策文档（`docs/adr/` 相关编号、`CONTEXT.md` 词条），若决策已实现、语义已演进或描述与代码现状不符，先更新文档再提交（文档遵循 `docs:` 单独提交惯例）。
- 发布流程（release-management skill）中的「全量质量门」即本约定，必须全绿。

### Rust

- 使用 2024 edition。
- 模块组织：按功能拆分为 `config`、`db`、`entity`、`cron`、`routes`、`middleware` 等模块，单文件 ≤ 1000 行（超出按域拆分，门面重导出保持既有路径不变）。
- 统一 API 响应：使用 `src/response.rs` 中的 `Response<T>`，字段名为 `code`、`msg`、`data`，成功时 `code` 为字符串 `"0"`；错误码常量和错误响应 helper（`bad_request`/`not_found` 等）也定义在 `src/response.rs`，路由层统一使用；错误消息使用中文。
- 错误处理：后端大量使用 `anyhow::Result` 与自定义 `SchedulerError`；HTTP 层返回统一 JSON 错误响应。
- 数据库：实体使用 SeaORM 的派生宏；迁移逻辑写在 `src/db.rs` 的 `migrate()` 中，启动时自动建表并通过 `schema_migrations` 版本表做增量迁移（编号规则见 `docs/agents/build-and-test.md`）。
- 日志：结构化 JSON 日志，开发时同时输出到 stdout 与 `logs/app.YYYY-MM-DD`。

### 前端

- TypeScript 严格模式开启，`noUnusedLocals`、`noUnusedParameters` 开启。
- 路径别名 `@/` 指向 `web/src/`。
- UI 组件按 shadcn/ui 约定放在 `web/src/components/ui/`，通过 `cn()` 合并 Tailwind 类名。
- 页面放在 `web/src/pages/`，通用布局在 `web/src/components/layout.tsx`。
- Hooks 放在 `web/src/hooks/`，API 封装在 `web/src/lib/api.ts`（`ApiResponse<T>` 对应后端 `code/msg/data`，`code !== "0"` 抛 `ApiError`）。
- 使用函数式组件与 Hooks，状态管理以 TanStack Query 为主，Zustand 用于主题等全局状态（`web/src/hooks/use-theme.ts`）。
- UI 文案为中文（如「定时任务」、「设置」、「操作成功」）。
- 弹窗内容可滚动时，标题栏与底部操作区必须固定高度、固定位置且不参与滚动；仅中间内容区负责滚动。
- 超长文本展示规范：单行截断一律用 `MidEllipsis` 组件（`web/src/components/mid-ellipsis.tsx`，按实测宽度自适应保留首尾、中间收起为 `…`），禁止新增 `truncate`/`text-ellipsis` 尾部省略；固定字符数截断（图表轴等 SVG 场景）用 `lib/utils.ts` 的 `middleEllipsis()`。多行 `line-clamp`、富文本混合节点与 shadcn 基础组件内部（sidebar 菜单、select 触发器）不适用中间省略，保留原样。

## 常见问题与注意事项

1. **前端构建产物必须存在**: 发布构建时，`rust-embed` 会内嵌 `web/dist`。如果本地手动构建后端，请先执行 `cd web && pnpm build`；Dockerfile 中已自动处理。
2. **定时任务需要注册 Handler 才会执行**: 数据库中的任务若没有对应注册的 Handler，加载时会被跳过，且不会出现在任务列表中；新增内置任务须同时加种子行与 `register_handler`（见 `docs/agents/cron.md`）。
3. **环境变量不会自动加载 `.env`**: 当前未集成 `dotenv`，运行前请确保环境变量已导出。
4. **日志清理**: 后端启动后会启动一个后台任务，每天清理一次日志目录中超过 30 天的文件（按修改时间判断，不区分文件类型，不要往日志目录放其他文件）。
5. **Biome 配置**: `web/biome.json` 已存在，`pnpm lint` 与 `pnpm format` 使用该配置（tab 缩进、双引号、100 列最大宽度）。
6. **健康检查**: `/api/healthz` 只表示进程存活，不检查数据库等依赖。

## Agent skills

### Release management

正式发布生产/开源/Release 版本（改版本号 → 打 `v*` tag → push → Release CI）的完整流程。
见 `.agents/skills/release-management/SKILL.md`。注意与 FRP 本地部署（`.deploy/deploy.sh`）区分。

### Local deploy

本地部署到 FRP 阿里云生产环境（交叉编译 musl + `.deploy/deploy.sh`）。见 `.agents/skills/local-deploy/SKILL.md`。

### Merge branches

把已完成的特性/修复分支逐个合入 main、过质量门并清理 worktree。见 `.agents/skills/merge-branches/SKILL.md`。

### Issue tracker

Issues and specs live as markdown files under `.scratch/<feature-slug>/` in this repo. See `docs/agents/issue-tracker.md`.

### Triage labels

Five canonical roles with default label strings (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: root `CONTEXT.md` + `docs/adr/`. See `docs/agents/domain.md`.
