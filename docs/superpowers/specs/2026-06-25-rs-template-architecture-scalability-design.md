# rs-template 架构可扩展性与性能优化设计

## 1. 背景与目标

**项目**：rs-template —— Rust（Axum + SeaORM + SQLite）+ React（Vite）单体应用模板。  
**约束**：保留 SQLite 数据库栈不变。  
**目标**：在不更换数据库的前提下，提升后端调度模块的可扩展性与执行性能，并改善代码组织，使其能支撑更多定时任务、更长任务执行时间以及更高并发触发。

## 2. 当前架构评估

### 2.1 整体结构

当前模块划分清晰：

- `config`：环境变量配置。
- `db`：SeaORM 连接与启动迁移。
- `state`：轻量 `AppState { db, scheduler }`。
- `routes`：Axum 路由处理器，较薄。
- `scheduler`：集中了几乎所有调度相关逻辑。
- `entity`：SeaORM 实体定义。

整体属于**职责明确但实现集中**的架构，适合快速启动，但调度核心已经增长到约 780 行，承担了过多职责。

### 2.2 关键问题

| 问题 | 影响 | 优先级 |
|---|---|---|
| `SchedulerManager` 启动后未调用 `load_from_db()` | 数据库中的定时任务不会被加载到内存调度器，系统功能基本不可用 | 🔴 关键 |
| `scheduler.rs` 职责过重 | 解析、持久化、运行时调度、任务执行全部耦合，难以测试和扩展 | 🟠 高 |
| 任务触发与执行同线程 | 单个耗时任务会阻塞调度器，影响其他任务准时触发 | 🟠 高 |
| `JobHandler` 无应用上下文 | 业务 handler 无法访问 `AppState` / `db`，必须自己建立连接或使用全局变量 | 🟠 高 |
| `list_jobs_detailed()` 逐条查询 DB | N 次单条查询替代一次批量查询，任务数多时会成为瓶颈 | 🟠 高 |
| 错误响应全部 HTTP 200 | 前端难以区分成功与失败，用户体验差 | 🟡 中 |
| 迁移逻辑无版本控制 | 后续 schema 变更会堆积为无记录的 `ALTER` 语句 | 🟡 中 |
| 无 `.dockerignore` | 构建时可能将宿主机 `node_modules` 复制进镜像 | 🟡 中 |

## 3. 推荐方案：调度与执行解耦

### 3.1 核心思想

将调度模块拆分为**三个独立层**：

1. **解析层（纯函数）**：负责 cron / `@every` 表达式的解析与下一次执行时间计算。
2. **持久层（Repository）**：负责所有 SeaORM 数据库读写，隔离 SQL/ORM 细节。
3. **运行层（Scheduler）**：只负责内存中的 `tokio-cron-scheduler` 运行时和触发事件分发。

并在触发与执行之间引入**异步任务队列**，实现：

- 调度器只负责"到点触发"；
- 触发后把任务投递到队列；
- Worker 池消费队列并执行任务；
- 单个长任务不会阻塞调度器。

### 3.2 目标架构

```text
src/
├── cron/
│   ├── mod.rs           # 模块导出与公共类型
│   ├── parser.rs        # parse_expression, compute_next_run, compute_frequency_secs
│   ├── repository.rs    # CronJobRepository：所有 DB 操作
│   ├── scheduler.rs     # SchedulerRuntime：tokio-cron-scheduler 封装
│   └── worker.rs        # JobWorker：任务执行队列与并发控制
├── routes/
│   └── cron_jobs.rs     # 更薄，仅负责 HTTP 契约转换
├── entity/
│   └── cron_job.rs      # 保持不变
└── lib.rs               # 初始化时注册 handler -> 加载 DB -> 启动 worker
```

### 3.3 关键接口设计

#### Handler 签名改进

当前 handler 无法访问应用状态：

```rust
pub type JobHandler = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;
```

改进为接收 `AppState`（或只读上下文）：

```rust
#[derive(Clone)]
pub struct JobContext {
    pub db: DatabaseConnection,
    // 未来可扩展：config, http_client, cache 等
}

pub type JobHandler = Arc<
    dyn Fn(JobContext) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync,
>;
```

这样业务 handler 可以直接使用 `ctx.db` 进行数据库操作，无需自行建连。

#### Repository 接口

```rust
#[async_trait::async_trait]
pub trait CronJobRepository: Send + Sync + Clone {
    async fn list_active(&self) -> Result<Vec<cron_job::Model>, DbErr>;
    async fn find_by_name(&self, name: &str) -> Result<Option<cron_job::Model>, DbErr>;
    async fn insert(&self, job: &JobDefinition) -> Result<(), DbErr>;
    async fn update(&self, name: &str, job: &JobDefinition) -> Result<(), DbErr>;
    async fn update_run_times(&self, name: &str, last: DateTime<Local>, next: DateTime<Local>) -> Result<(), DbErr>;
    async fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), DbErr>;
    async fn soft_delete(&self, name: &str) -> Result<(), DbErr>;
}
```

实现为 `SeaOrmCronJobRepository { db: DatabaseConnection }`。

#### Worker 队列

```rust
pub struct JobWorker {
    tx: mpsc::Sender<JobInvocation>,
    semaphore: Arc<Semaphore>,
}

struct JobInvocation {
    name: String,
    handler: JobHandler,
    ctx: JobContext,
}
```

- 使用 `tokio::sync::mpsc` 作为任务通道；
- 使用 `tokio::sync::Semaphore` 控制最大并发执行数（例如 10）；
- 执行完成后自动更新 `last_run_at` / `next_run_at`。

### 3.4 初始化流程

```rust
// lib.rs::init
let db = db::connect(&config.database_url).await?;
let repo = SeaOrmCronJobRepository::new(db.clone());

let scheduler = SchedulerRuntime::new(db.clone()).await?;
let worker = JobWorker::new(db.clone(), /* max_concurrent */ 10);

// 注册业务 handler（示例）
scheduler.register_handler("cleanup_logs", Arc::new(|ctx| Box::pin(async move {
    logs_cleanup::run_with_context(ctx).await;
}))).await;

// 从数据库加载任务
scheduler.load_from_db(&repo).await?;

let state = AppState {
    db: db.clone(),
    scheduler,
    worker,
};
```

## 4. 具体改动清单

### 4.1 后端代码重构

| 文件/模块 | 改动 |
|---|---|
| `src/scheduler.rs` | 拆分为 `src/cron/*.rs`，原文件删除或变为薄包装 |
| `src/cron/parser.rs` | 迁移 `parse_expression`、`compute_next_run`、`compute_frequency_secs` 及测试 |
| `src/cron/repository.rs` | 新建，封装所有 SeaORM 操作 |
| `src/cron/scheduler.rs` | 新建，仅保留内存调度器运行时 |
| `src/cron/worker.rs` | 新建，实现任务队列与并发控制 |
| `src/state.rs` | `AppState` 增加 `worker` 字段 |
| `src/lib.rs` | 初始化时注册 handler、加载 DB、启动 worker |
| `src/routes/cron_jobs.rs` | 调用 repository / scheduler 接口，减少直接 DB 操作 |
| `src/entity/cron_job.rs` | 考虑为 `name` 添加索引；将 `group` 改为 `Option<String>` 或迁移默认值 |

### 4.2 启动时加载任务

在 `lib.rs::init` 中显式调用：

```rust
scheduler.load_from_db(&repo).await?;
scheduler.start().await?;
worker.start().await?;
```

### 4.3 数据库索引

在 `src/db.rs::migrate` 中为 `cron_jobs.name` 添加唯一索引（若 SeaORM 未自动创建）：

```rust
// 若实体定义未包含索引，可在迁移中补充
manager
    .create_index(
        Index::create()
            .if_not_exists()
            .name("idx_cron_jobs_name")
            .table(CronJob::Table)
            .col(CronJob::Name)
            .unique()
            .to_owned(),
    )
    .await?;
```

### 4.4 批量查询优化

将 `list_jobs_detailed` 从逐条查询改为一次性查询：

```rust
pub async fn list_jobs_detailed(&self) -> Result<Vec<JobInfo>, SchedulerError> {
    let mut jobs = self.list_jobs().await;
    let names: Vec<String> = jobs.iter().map(|j| j.name.clone()).collect();

    let models = CronJob::find()
        .filter(cron_job::Column::Name.is_in(names))
        .all(&self.db)
        .await?;

    let model_map: HashMap<_, _> = models.into_iter().map(|m| (m.name.clone(), m)).collect();
    for job in &mut jobs {
        if let Some(model) = model_map.get(&job.name) {
            job.last_run_at = model.last_run_at;
            job.next_run_at = model.next_run_at;
            job.updated_at = model.updated_at;
        }
    }
    Ok(jobs)
}
```

### 4.5 迁移安全性

- 为 `cron_jobs.group` 的 `ALTER TABLE` 添加 `DEFAULT ''`，避免已有行出现 `NULL` 与 `String` 模型不匹配。
- 添加 `schema_migrations` 表记录已执行的迁移版本。
- 将迁移步骤包裹在事务中（SQLite 支持 DDL 事务）。

### 4.6 前端配套改进

| 改动 | 说明 |
|---|---|
| 创建 `web/src/lib/api.ts` | 统一 `ky` 实例和 `{ code, msg, data }` 拆包逻辑 |
| 创建 `web/src/hooks/use-cron-jobs.ts` | 将 TanStack Query 逻辑集中 |
| 检查 `code !== "0"` | 让 mutation 在业务失败时正确报错 |
| 移除未使用依赖 | 若短期内不实现表单校验，移除 `react-hook-form`、`@hookform/resolvers`、`zod` |

### 4.7 构建与 CI

| 改动 | 说明 |
|---|---|
| 新增 `.dockerignore` | 排除 `target/`、`web/node_modules/`、`web/dist/`、`db/`、`logs/`、`.git/` |
| 移除 Dockerfile 中硬编码 secrets | 改为由 CI secrets 注入 |
| CI 增加测试步骤 | `cargo test`、`cargo clippy`、`cd web && pnpm build` |
| CI 推送 SHA 标签 | 保留 `:latest` 同时推送 `<commit-sha>` 标签 |

## 5. 预期收益

| 指标 | 当前 | 优化后 |
|---|---|---|
| 调度器可用性 | 启动后无任务加载，功能不可用 | 启动即加载并调度数据库任务 |
| 任务执行并发 | 单线程，长任务阻塞调度器 | 独立 worker 池，可控并发 |
| 数据库查询 | `list_jobs_detailed` 为 N+1 | 一次批量查询 |
| 可测试性 | `scheduler.rs` 难以单元测试 | 解析、持久化、运行时三层可独立测试 |
| Handler 能力 | 无状态、无法访问 DB | 可访问 `JobContext` 与数据库 |
| 代码可维护性 | 780 行单文件 | 职责分离，模块边界清晰 |

## 6. 风险与缓解

| 风险 | 影响 | 缓解措施 |
|---|---|---|
| 重构引入回归 | 高 | 保持现有 12 个测试通过，并新增 scheduler / repository / worker 单元测试 |
| SQLite 并发写入瓶颈 | 中 | 使用 WAL 模式（已开启），将高频状态更新合并为事务批量写入 |
| Handler 签名变更 | 中 | 属于 breaking change，所有注册点需同步更新；在 `register_handler` 提供适配辅助 |
| 前端错误处理改动 | 低 | 先统一 API 层，再逐步替换页面中的调用 |

## 7. 不改 SQLite 的边界说明

SQLite 在单实例场景下表现优异，但其并发写入受限于文件锁。本方案所有优化都发生在**单进程内部**：

- 不引入外部消息队列或缓存；
- 不改为多实例共享数据库的架构；
- 通过 worker 池和批量查询提升单实例吞吐。

如果未来任务量达到每秒数百次写入或需要多机部署，那才是考虑迁移到 PostgreSQL 的合适时机。

## 8. 下一步

本设计文档确认后，可进入 `writing-plans` 阶段，生成按文件级别的具体实施计划与测试策略。
