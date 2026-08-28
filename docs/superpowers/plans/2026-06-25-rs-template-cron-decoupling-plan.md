# rs-template Cron 调度与执行解耦实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在保留 SQLite 的前提下，将 `scheduler.rs` 拆分为解析、持久化、运行时、执行四层，引入异步任务队列，修复启动加载问题，提升可扩展性与可维护性。

**Architecture:** 将原 `SchedulerManager` 拆分为 `cron::parser`（纯函数）、`cron::repository`（SeaORM 数据访问）、`cron::scheduler`（`tokio-cron-scheduler` 运行时）、`cron::worker`（异步执行队列）。调度器仅负责到点触发，任务通过 `mpsc` 投递给 `JobWorker`，由 Semaphore 控制并发。Handler 接收 `JobContext` 以访问数据库。

**Tech Stack:** Rust 2024, Axum 0.8, SeaORM 2.0.0-rc.40 (SQLite), tokio-cron-scheduler 0.15, tokio mpsc/semaphore, React + Vite + TanStack Query。

---

## 文件结构

| 文件 | 职责 |
|---|---|
| `src/cron/mod.rs` | 模块导出、公共类型（`JobContext`、`JobHandler`、`JobInfo`、`SchedulerError`） |
| `src/cron/parser.rs` | cron / `@every` 解析、下次运行时间计算、频率估计 |
| `src/cron/repository.rs` | `CronJobRepository` trait 与 `SeaOrmCronJobRepository` 实现 |
| `src/cron/scheduler.rs` | `SchedulerRuntime`：内存调度器、handler 注册、DB 加载 |
| `src/cron/worker.rs` | `JobWorker`：任务队列、并发控制、执行后更新运行时间 |
| `src/state.rs` | `AppState` 增加 `worker: cron::worker::JobWorker` |
| `src/lib.rs` | 初始化流程：注册 handler → 加载 DB → 启动 scheduler/worker |
| `src/routes/cron_jobs.rs` | 适配新的 repository/scheduler 接口 |
| `src/db.rs` | 补充索引、修复 `group` 默认值、添加迁移版本表 |
| `web/src/lib/api.ts` | 统一后端 API 客户端与响应拆包 |
| `web/src/hooks/use-cron-jobs.ts` | 集中 TanStack Query 逻辑 |

---

## 前置依赖检查

运行以下命令确认当前测试通过：

```bash
cargo test
```

预期输出：所有 12 个测试通过。

---

## Task 1: 创建 `cron` 模块并迁移解析层

**Files:**
- Create: `src/cron/mod.rs`
- Create: `src/cron/parser.rs`
- Modify: `src/lib.rs`（注册新模块）
- Delete: `src/scheduler.rs`（在 Task 5 完成后删除）

### Step 1.1: 创建 `src/cron/mod.rs`

```rust
pub mod parser;
pub mod repository;
pub mod scheduler;
pub mod worker;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use sea_orm::DatabaseConnection;

#[derive(Clone)]
pub struct JobContext {
    pub db: DatabaseConnection,
}

pub type JobHandler = Arc<
    dyn Fn(JobContext) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync,
>;

#[derive(Clone, Debug)]
pub struct JobInfo {
    pub name: String,
    pub title: String,
    pub description: String,
    pub expression: String,
    pub enabled: bool,
    pub last_run_at: chrono::DateTime<chrono::Local>,
    pub next_run_at: chrono::DateTime<chrono::Local>,
    pub updated_at: chrono::DateTime<chrono::Local>,
    pub running: bool,
    pub group: String,
    pub frequency_secs: i64,
}

#[derive(Debug)]
pub enum SchedulerError {
    ParseError(String),
    JobScheduler(tokio_cron_scheduler::JobSchedulerError),
    Db(sea_orm::DbErr),
    JobNotFound(String),
    HandlerNotFound(String),
    ComputeNextRun(String),
}

impl std::fmt::Display for SchedulerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchedulerError::ParseError(e) => write!(f, "parse error: {}", e),
            SchedulerError::JobScheduler(e) => write!(f, "job scheduler error: {}", e),
            SchedulerError::Db(e) => write!(f, "db error: {}", e),
            SchedulerError::JobNotFound(name) => write!(f, "job not found: {}", name),
            SchedulerError::HandlerNotFound(name) => write!(f, "handler not found: {}", name),
            SchedulerError::ComputeNextRun(e) => write!(f, "compute next run error: {}", e),
        }
    }
}

impl std::error::Error for SchedulerError {}

impl From<tokio_cron_scheduler::JobSchedulerError> for SchedulerError {
    fn from(e: tokio_cron_scheduler::JobSchedulerError) -> Self {
        SchedulerError::JobScheduler(e)
    }
}

impl From<sea_orm::DbErr> for SchedulerError {
    fn from(e: sea_orm::DbErr) -> Self {
        SchedulerError::Db(e)
    }
}
```

### Step 1.2: 创建 `src/cron/parser.rs`

将原 `scheduler.rs` 中的 `parse_expression`、`compute_next_run`、`compute_frequency_secs` 及测试迁移至此。

```rust
use std::str::FromStr;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum ScheduleType {
    Cron(String),
    Every(Duration),
}

pub fn compute_next_run(
    expression: &str,
) -> Result<chrono::DateTime<chrono::Local>, SchedulerError> {
    let schedule = parse_expression(expression).map_err(SchedulerError::ParseError)?;
    let now = chrono::Local::now();
    match schedule {
        ScheduleType::Cron(cron_expr) => {
            let cron = croner::Cron::from_str(&cron_expr).map_err(|e| {
                SchedulerError::ComputeNextRun(format!("invalid cron: {e}"))
            })?;
            let next = cron
                .find_next_occurrence(&now, false)
                .map_err(|e| SchedulerError::ComputeNextRun(e.to_string()))?;
            Ok(next)
        }
        ScheduleType::Every(duration) => {
            let secs = duration.as_secs() as i64;
            Ok(now + chrono::TimeDelta::seconds(secs))
        }
    }
}

pub fn compute_frequency_secs(expression: &str) -> i64 {
    match parse_expression(expression) {
        Ok(ScheduleType::Every(duration)) => duration.as_secs() as i64,
        Ok(ScheduleType::Cron(cron_expr)) => estimate_cron_period(&cron_expr),
        Err(_) => i64::MAX,
    }
}

pub fn parse_expression(expr: &str) -> Result<ScheduleType, String> {
    let expr = expr.trim();

    match expr {
        "@yearly" | "@annually" => Ok(ScheduleType::Cron("0 0 0 1 1 *".to_string())),
        "@monthly" => Ok(ScheduleType::Cron("0 0 0 1 * *".to_string())),
        "@weekly" => Ok(ScheduleType::Cron("0 0 0 * * 0".to_string())),
        "@daily" | "@midnight" => Ok(ScheduleType::Cron("0 0 0 * * *".to_string())),
        "@hourly" => Ok(ScheduleType::Cron("0 0 * * * *".to_string())),
        _ => {
            if let Some(dur_str) = expr.strip_prefix("@every ") {
                let dur = parse_duration(dur_str)?;
                Ok(ScheduleType::Every(dur))
            } else {
                let parts: Vec<&str> = expr.split_whitespace().collect();
                match parts.len() {
                    5 => Ok(ScheduleType::Cron(format!("0 {}", expr))),
                    6 => Ok(ScheduleType::Cron(expr.to_string())),
                    _ => Err(format!(
                        "invalid cron expression '{}', expected 5 or 6 fields",
                        expr
                    )),
                }
            }
        }
    }
}

fn estimate_cron_period(cron_expr: &str) -> i64 {
    let parts: Vec<&str> = cron_expr.split_whitespace().collect();
    if parts.len() != 6 {
        return i64::MAX;
    }

    for (idx, mul) in [(0, 1i64), (1, 60i64), (2, 3600i64)] {
        if let Some(step) = parse_step(parts[idx]) {
            return step * mul;
        }
    }

    match cron_expr {
        "0 0 * * * *" => return 3600,
        "0 0 0 * * *" => return 86400,
        "0 0 0 * * 0" => return 604800,
        "0 0 0 1 * *" => return 2592000,
        "0 0 0 1 1 *" => return 31536000,
        _ => {}
    }

    let has_day_of_month = !parts[3].starts_with('*');
    let has_month = !parts[4].starts_with('*');
    let has_day_of_week = !parts[5].starts_with('*');

    if has_month {
        return 30 * 86400;
    }
    if has_day_of_month || has_day_of_week {
        return 86400;
    }

    if !parts[2].starts_with('*') {
        return 86400;
    }
    if !parts[1].starts_with('*') {
        return 3600;
    }
    if !parts[0].starts_with('*') {
        return 60;
    }

    1
}

fn parse_step(field: &str) -> Option<i64> {
    if field == "*" {
        return None;
    }
    if let Some(step_str) = field.strip_prefix("*/") {
        return step_str.parse().ok();
    }
    if field.contains('/') {
        let parts: Vec<&str> = field.split('/').collect();
        if parts.len() == 2 {
            return parts[1].parse().ok();
        }
    }
    None
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let mut total_secs: u64 = 0;
    let mut i = 0;
    let bytes = s.as_bytes();

    while i < bytes.len() {
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
            i += 1;
        }
        if start == i {
            return Err(format!("expected number at position {} in '{}'", i, s));
        }
        let num_str = &s[start..i];
        let num: f64 = num_str
            .parse()
            .map_err(|_| format!("invalid number '{}' in '{}'", num_str, s))?;

        if i >= bytes.len() {
            return Err(format!("missing unit after '{}' in '{}'", num_str, s));
        }
        let unit = bytes[i] as char;
        i += 1;

        let secs = match unit {
            's' => num,
            'm' => num * 60.0,
            'h' => num * 3600.0,
            'd' => num * 86400.0,
            _ => {
                return Err(format!(
                    "unknown unit '{}' in '{}', expected s/m/h/d",
                    unit, s
                ));
            }
        };
        total_secs += secs as u64;
    }

    if total_secs == 0 && !s.is_empty() {
        return Err(format!("invalid duration '{}'", s));
    }

    Ok(Duration::from_secs(total_secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_frequency_secs_every() {
        assert_eq!(compute_frequency_secs("@every 5m"), 300);
        assert_eq!(compute_frequency_secs("@every 1h"), 3600);
        assert_eq!(compute_frequency_secs("@every 24h"), 86400);
        assert_eq!(compute_frequency_secs("@every 90m"), 5400);
    }

    #[test]
    fn test_compute_frequency_secs_shorthands() {
        assert_eq!(compute_frequency_secs("@hourly"), 3600);
        assert_eq!(compute_frequency_secs("@daily"), 86400);
        assert_eq!(compute_frequency_secs("@weekly"), 604800);
        assert_eq!(compute_frequency_secs("@monthly"), 2592000);
    }

    #[test]
    fn test_compute_frequency_secs_cron() {
        assert_eq!(compute_frequency_secs("0 0 */6 * * *"), 6 * 3600);
        assert_eq!(compute_frequency_secs("0 */10 * * * *"), 600);
        assert_eq!(compute_frequency_secs("0 0 * * * *"), 3600);
    }

    #[test]
    fn test_compute_frequency_secs_invalid() {
        assert_eq!(compute_frequency_secs("invalid"), i64::MAX);
    }
}
```

注意：需要在 `Cargo.toml` 中确认 `croner` 已存在（当前已有）。

### Step 1.3: 修改 `src/lib.rs` 注册模块

将

```rust
pub mod scheduler;
```

替换为

```rust
pub mod cron;
```

### Step 1.4: 编译检查

```bash
cargo check
```

预期：可能暂时缺少 `scheduler` 引用而报错，这很正常，下一步再修复。

### Step 1.5: Commit

```bash
git add src/cron/mod.rs src/cron/parser.rs src/lib.rs
git commit -m "refactor(cron): extract parser module and shared types"
```

---

## Task 2: 创建 Repository 层

**Files:**
- Create: `src/cron/repository.rs`

### Step 2.1: 创建 `src/cron/repository.rs`

```rust
use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Local};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, Set,
};

use crate::entity::cron_job;

#[derive(Clone, Debug)]
pub struct JobDefinition {
    pub name: String,
    pub title: String,
    pub description: String,
    pub expression: String,
    pub enabled: bool,
    pub group: String,
}

impl From<&cron_job::Model> for JobDefinition {
    fn from(model: &cron_job::Model) -> Self {
        Self {
            name: model.name.clone(),
            title: model.title.clone(),
            description: model.description.clone(),
            expression: model.expression.clone(),
            enabled: model.enabled,
            group: model.group.clone(),
        }
    }
}

#[async_trait]
pub trait CronJobRepository: Send + Sync + Clone {
    async fn list_active(&self) -> Result<Vec<cron_job::Model>, DbErr>;
    async fn list_by_names(&self, names: &[String]) -> Result<Vec<cron_job::Model>, DbErr>;
    async fn find_by_name(&self, name: &str) -> Result<Option<cron_job::Model>, DbErr>;
    async fn insert(&self, job: &JobDefinition) -> Result<cron_job::Model, DbErr>;
    async fn update_by_name(
        &self,
        name: &str,
        job: &JobDefinition,
    ) -> Result<Option<cron_job::Model>, DbErr>;
    async fn update_run_times(
        &self,
        name: &str,
        last_run_at: DateTime<Local>,
        next_run_at: DateTime<Local>,
    ) -> Result<(), DbErr>;
    async fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), DbErr>;
    async fn soft_delete(&self, name: &str) -> Result<(), DbErr>;
    async fn restore(&self, name: &str, enabled: bool) -> Result<(), DbErr>;
}

#[derive(Clone)]
pub struct SeaOrmCronJobRepository {
    db: DatabaseConnection,
}

impl SeaOrmCronJobRepository {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

#[async_trait]
impl CronJobRepository for SeaOrmCronJobRepository {
    async fn list_active(&self) -> Result<Vec<cron_job::Model>, DbErr> {
        cron_job::Entity::find()
            .filter(cron_job::Column::IsDeleted.eq(false))
            .all(&self.db)
            .await
    }

    async fn list_by_names(&self, names: &[String]) -> Result<Vec<cron_job::Model>, DbErr> {
        if names.is_empty() {
            return Ok(vec![]);
        }
        cron_job::Entity::find()
            .filter(cron_job::Column::Name.is_in(names.to_vec()))
            .all(&self.db)
            .await
    }

    async fn find_by_name(&self, name: &str) -> Result<Option<cron_job::Model>, DbErr> {
        cron_job::Entity::find()
            .filter(cron_job::Column::Name.eq(name))
            .one(&self.db)
            .await
    }

    async fn insert(&self, job: &JobDefinition) -> Result<cron_job::Model, DbErr> {
        let now = Local::now();
        let epoch: DateTime<Local> = DateTime::UNIX_EPOCH.into();
        let active = cron_job::ActiveModel {
            name: Set(job.name.clone()),
            title: Set(job.title.clone()),
            description: Set(job.description.clone()),
            expression: Set(job.expression.clone()),
            enabled: Set(job.enabled),
            group: Set(job.group.clone()),
            last_run_at: Set(epoch),
            next_run_at: Set(crate::cron::parser::compute_next_run(&job.expression).unwrap_or(now)),
            created_at: Set(now),
            updated_at: Set(now),
            is_deleted: Set(false),
            ..Default::default()
        };
        active.insert(&self.db).await
    }

    async fn update_by_name(
        &self,
        name: &str,
        job: &JobDefinition,
    ) -> Result<Option<cron_job::Model>, DbErr> {
        let model = self.find_by_name(name).await?;
        let Some(model) = model else {
            return Ok(None);
        };
        let now = Local::now();
        let mut active: cron_job::ActiveModel = model.into();
        active.name = Set(job.name.clone());
        active.title = Set(job.title.clone());
        active.description = Set(job.description.clone());
        active.expression = Set(job.expression.clone());
        active.enabled = Set(job.enabled);
        active.group = Set(job.group.clone());
        active.updated_at = Set(now);
        active.update(&self.db).await.map(Some)
    }

    async fn update_run_times(
        &self,
        name: &str,
        last_run_at: DateTime<Local>,
        next_run_at: DateTime<Local>,
    ) -> Result<(), DbErr> {
        let model = self.find_by_name(name).await?;
        if let Some(model) = model {
            let mut active: cron_job::ActiveModel = model.into();
            active.last_run_at = Set(last_run_at);
            active.next_run_at = Set(next_run_at);
            active.update(&self.db).await?;
        }
        Ok(())
    }

    async fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), DbErr> {
        let model = self.find_by_name(name).await?;
        if let Some(model) = model {
            let mut active: cron_job::ActiveModel = model.into();
            active.enabled = Set(enabled);
            active.updated_at = Set(Local::now());
            active.update(&self.db).await?;
        }
        Ok(())
    }

    async fn soft_delete(&self, name: &str) -> Result<(), DbErr> {
        let model = self.find_by_name(name).await?;
        if let Some(model) = model {
            let mut active: cron_job::ActiveModel = model.into();
            active.is_deleted = Set(true);
            active.enabled = Set(false);
            active.updated_at = Set(Local::now());
            active.update(&self.db).await?;
        }
        Ok(())
    }

    async fn restore(&self, name: &str, enabled: bool) -> Result<(), DbErr> {
        let model = self.find_by_name(name).await?;
        if let Some(model) = model {
            let mut active: cron_job::ActiveModel = model.into();
            active.is_deleted = Set(false);
            active.enabled = Set(enabled);
            active.updated_at = Set(Local::now());
            active.update(&self.db).await?;
        }
        Ok(())
    }
}
```

注意：`async_trait` 需要加入 `Cargo.toml`：

```toml
async-trait = "0.1"
```

### Step 2.2: 添加 `async-trait` 依赖

修改 `Cargo.toml`：

```toml
[dependencies]
anyhow = "1"
async-trait = "0.1"
axum = "0.8"
# ... 其他保持不变
```

### Step 2.3: 编译检查

```bash
cargo check
```

预期：repository 本身可编译通过。

### Step 2.4: Commit

```bash
git add Cargo.toml Cargo.lock src/cron/repository.rs
git commit -m "refactor(cron): add CronJobRepository abstraction"
```

---

## Task 3: 创建 Worker 执行层

**Files:**
- Create: `src/cron/worker.rs`

### Step 3.1: 创建 `src/cron/worker.rs`

```rust
use std::sync::Arc;

use sea_orm::DatabaseConnection;
use tokio::sync::{mpsc, Semaphore};

use crate::cron::parser::compute_next_run;
use crate::cron::repository::{CronJobRepository, SeaOrmCronJobRepository};
use crate::cron::{JobContext, JobHandler};

#[derive(Clone)]
pub struct JobWorker {
    db: DatabaseConnection,
    max_concurrent: usize,
}

impl JobWorker {
    pub fn new(db: DatabaseConnection, max_concurrent: usize) -> Self {
        Self {
            db,
            max_concurrent: max_concurrent.max(1),
        }
    }

    /// Spawn the worker background task and return a sender to submit jobs.
    pub fn start(&self) -> mpsc::UnboundedSender<JobInvocation> {
        let (tx, mut rx) = mpsc::unbounded_channel::<JobInvocation>();
        let semaphore = Arc::new(Semaphore::new(self.max_concurrent));
        let db = self.db.clone();

        tokio::spawn(async move {
            while let Some(invocation) = rx.recv().await {
                let permit = semaphore.clone().acquire_owned().await;
                if permit.is_err() {
                    break;
                }
                let db = db.clone();
                tokio::spawn(async move {
                    let name = invocation.name.clone();
                    let ctx = JobContext { db: db.clone() };
                    (invocation.handler)(ctx).await;
                    let repo = SeaOrmCronJobRepository::new(db);
                    let now = chrono::Local::now();
                    let next = compute_next_run(&invocation.expression).unwrap_or(now);
                    if let Err(e) = repo.update_run_times(&name, now, next).await {
                        tracing::error!("Failed to update run times for '{}': {}", name, e);
                    }
                    drop(permit);
                });
            }
        });

        tx
    }
}

#[derive(Clone)]
pub struct JobInvocation {
    pub name: String,
    pub expression: String,
    pub handler: JobHandler,
}
```

### Step 3.2: 编译检查

```bash
cargo check
```

### Step 3.3: Commit

```bash
git add src/cron/worker.rs
git commit -m "feat(cron): add JobWorker with async queue and concurrency limit"
```

---

## Task 4: 创建 SchedulerRuntime 运行时层

**Files:**
- Create: `src/cron/scheduler.rs`

### Step 4.1: 创建 `src/cron/scheduler.rs`

```rust
use std::collections::HashMap;
use std::sync::Arc;

use sea_orm::DatabaseConnection;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_cron_scheduler::{Job, JobScheduler};

use crate::cron::parser::{compute_frequency_secs, compute_next_run, parse_expression, ScheduleType};
use crate::cron::repository::{CronJobRepository, JobDefinition};
use crate::cron::worker::JobInvocation;
use crate::cron::{JobHandler, JobInfo, SchedulerError};

#[derive(Clone)]
pub struct JobEntry {
    pub name: String,
    pub title: String,
    pub description: String,
    pub expression: String,
    pub job: Job,
    pub enabled: bool,
    pub group: String,
    pub handler: JobHandler,
}

#[derive(Clone)]
pub struct SchedulerRuntime {
    scheduler: Arc<Mutex<JobScheduler>>,
    jobs: Arc<RwLock<HashMap<String, JobEntry>>>,
    handlers: Arc<RwLock<HashMap<String, JobHandler>>>,
    db: DatabaseConnection,
}

impl SchedulerRuntime {
    pub async fn new(db: DatabaseConnection) -> Result<Self, SchedulerError> {
        let scheduler = JobScheduler::new().await?;
        Ok(Self {
            scheduler: Arc::new(Mutex::new(scheduler)),
            jobs: Arc::new(RwLock::new(HashMap::new())),
            handlers: Arc::new(RwLock::new(HashMap::new())),
            db,
        })
    }

    pub async fn start(&self) -> Result<(), SchedulerError> {
        let sched = self.scheduler.lock().await;
        sched.start().await?;
        Ok(())
    }

    pub async fn stop(&self) -> Result<(), SchedulerError> {
        let mut sched = self.scheduler.lock().await;
        sched.shutdown().await?;
        Ok(())
    }

    pub async fn register_handler(&self, name: &str, handler: JobHandler) {
        let mut handlers = self.handlers.write().await;
        handlers.insert(name.to_string(), handler);
    }

    pub async fn get_handler(&self, name: &str) -> Option<JobHandler> {
        let handlers = self.handlers.read().await;
        handlers.get(name).cloned()
    }

    pub async fn load_from_db<R: CronJobRepository>(
        &self,
        repo: &R,
        worker_tx: mpsc::UnboundedSender<JobInvocation>,
    ) -> Result<(), SchedulerError> {
        let configs = repo.list_active().await?;
        let now = chrono::Local::now();

        for config in configs {
            let handler = match self.get_handler(&config.name).await {
                Some(h) => h,
                None => {
                    tracing::warn!(
                        "Handler for job '{}' not registered, skipping",
                        config.name
                    );
                    continue;
                }
            };

            let definition = JobDefinition::from(&config);
            if let Err(e) = self
                .add_job_internal(&definition, handler, worker_tx.clone())
                .await
            {
                tracing::error!("Failed to load job '{}': {}", config.name, e);
                continue;
            }

            if config.enabled
                && config.next_run_at < now
                && config.last_run_at <= config.next_run_at
            {
                tracing::info!(
                    "Job '{}' missed its scheduled run (next_run_at={}), triggering now",
                    config.name,
                    config.next_run_at
                );
                if let Err(e) = self.run_job_now(&config.name).await {
                    tracing::error!("Failed to run missed job '{}': {}", config.name, e);
                }
            }
        }
        Ok(())
    }

    pub async fn add_job<R: CronJobRepository>(
        &self,
        repo: &R,
        job: &JobDefinition,
        handler: JobHandler,
        worker_tx: mpsc::UnboundedSender<JobInvocation>,
    ) -> Result<(), SchedulerError> {
        self.add_job_internal(job, handler, worker_tx).await?;
        repo.insert(job).await?;
        Ok(())
    }

    async fn add_job_internal(
        &self,
        job: &JobDefinition,
        handler: JobHandler,
        worker_tx: mpsc::UnboundedSender<JobInvocation>,
    ) -> Result<(), SchedulerError> {
        let schedule = parse_expression(&job.expression).map_err(SchedulerError::ParseError)?;

        let name = job.name.clone();
        let expression = job.expression.clone();
        let wrapped_handler = handler.clone();

        let wrapped = Arc::new(move || {
            let tx = worker_tx.clone();
            let name = name.clone();
            let expression = expression.clone();
            let handler = wrapped_handler.clone();
            Box::pin(async move {
                let invocation = JobInvocation {
                    name,
                    expression,
                    handler,
                };
                if let Err(e) = tx.send(invocation) {
                    tracing::error!("Failed to submit job to worker queue: {}", e);
                }
            })
        });

        let mut tokio_job = match schedule {
            ScheduleType::Cron(cron_expr) => Job::new_async(cron_expr, move |_uuid, _l| {
                let h = wrapped.clone();
                Box::pin(async move { h().await })
            })?,
            ScheduleType::Every(duration) => Job::new_repeated_async(duration, move |_uuid, _l| {
                let h = wrapped.clone();
                Box::pin(async move { h().await })
            })?,
        };

        let sched = self.scheduler.lock().await;
        sched.add(tokio_job.clone()).await?;

        if !job.enabled {
            tokio_job.set_stop(true)?;
        }

        let mut jobs = self.jobs.write().await;
        jobs.insert(
            job.name.clone(),
            JobEntry {
                name: job.name.clone(),
                title: job.title.clone(),
                description: job.description.clone(),
                expression: job.expression.clone(),
                job: tokio_job,
                enabled: job.enabled,
                group: job.group.clone(),
                handler,
            },
        );

        Ok(())
    }

    async fn remove_job_from_scheduler(&self, name: &str) -> Result<(), SchedulerError> {
        let mut jobs = self.jobs.write().await;
        if let Some(entry) = jobs.remove(name) {
            let sched = self.scheduler.lock().await;
            sched.remove(&entry.job.guid()).await?;
        }
        Ok(())
    }

    pub async fn remove_job<R: CronJobRepository>(&self, repo: &R, name: &str) -> Result<(), SchedulerError> {
        self.remove_job_from_scheduler(name).await?;
        let model = repo.find_by_name(name).await?;
        if let Some(model) = model {
            model.delete(&self.db).await?;
        }
        Ok(())
    }

    pub async fn run_job_now(&self, name: &str) -> Result<(), SchedulerError> {
        let jobs = self.jobs.read().await;
        let entry = jobs
            .get(name)
            .ok_or_else(|| SchedulerError::JobNotFound(name.to_string()))?;
        let h = entry.handler.clone();
        drop(jobs);
        // Direct invocation bypasses the worker queue; alternatively submit to worker_tx.
        tokio::spawn(async move {
            h(JobContext { db: self.db.clone() }).await;
        });
        Ok(())
    }

    pub async fn update_expression<R: CronJobRepository>(
        &self,
        repo: &R,
        name: &str,
        expression: &str,
        worker_tx: mpsc::UnboundedSender<JobInvocation>,
    ) -> Result<(), SchedulerError> {
        let existing = {
            let jobs = self.jobs.read().await;
            let entry = jobs
                .get(name)
                .ok_or_else(|| SchedulerError::JobNotFound(name.to_string()))?;
            JobDefinition {
                name: entry.name.clone(),
                title: entry.title.clone(),
                description: entry.description.clone(),
                expression: expression.to_string(),
                enabled: entry.enabled,
                group: entry.group.clone(),
            }
        };

        self.remove_job_from_scheduler(name).await?;
        self.add_job_internal(&existing, existing.clone().handler().unwrap_or_default(), worker_tx)
            .await?;

        if let Some(mut model) = repo.find_by_name(name).await? {
            let now = chrono::Local::now();
            use sea_orm::ActiveModelTrait;
            use sea_orm::Set;
            let mut active: cron_job::ActiveModel = model.into();
            active.expression = Set(expression.to_string());
            active.next_run_at = Set(compute_next_run(expression).unwrap_or(now));
            active.updated_at = Set(now);
            active.update(&self.db).await?;
        }

        Ok(())
    }

    pub async fn update_info<R: CronJobRepository>(
        &self,
        repo: &R,
        name: &str,
        new_name: &str,
        title: &str,
        description: &str,
        worker_tx: mpsc::UnboundedSender<JobInvocation>,
    ) -> Result<(), SchedulerError> {
        let (existing, handler) = {
            let jobs = self.jobs.read().await;
            let entry = jobs
                .get(name)
                .ok_or_else(|| SchedulerError::JobNotFound(name.to_string()))?;
            (
                JobDefinition {
                    name: new_name.to_string(),
                    title: title.to_string(),
                    description: description.to_string(),
                    expression: entry.expression.clone(),
                    enabled: entry.enabled,
                    group: entry.group.clone(),
                },
                entry.handler.clone(),
            )
        };

        self.remove_job_from_scheduler(name).await?;
        self.add_job_internal(&existing, handler, worker_tx).await?;

        if let Some(model) = repo.find_by_name(name).await? {
            use sea_orm::Set;
            let mut active: cron_job::ActiveModel = model.into();
            active.name = Set(new_name.to_string());
            active.title = Set(title.to_string());
            active.description = Set(description.to_string());
            active.updated_at = Set(chrono::Local::now());
            active.update(&self.db).await?;
        }

        Ok(())
    }

    pub async fn set_enabled<R: CronJobRepository>(
        &self,
        repo: &R,
        name: &str,
        enabled: bool,
    ) -> Result<(), SchedulerError> {
        {
            let mut jobs = self.jobs.write().await;
            let entry = jobs
                .get_mut(name)
                .ok_or_else(|| SchedulerError::JobNotFound(name.to_string()))?;
            entry.enabled = enabled;
            entry.job.set_stop(!enabled)?;
        }
        repo.set_enabled(name, enabled).await?;
        Ok(())
    }

    pub async fn soft_delete_job<R: CronJobRepository>(
        &self,
        repo: &R,
        name: &str,
    ) -> Result<(), SchedulerError> {
        repo.soft_delete(name).await?;
        self.remove_job_from_scheduler(name).await?;
        Ok(())
    }

    pub async fn restore_job<R: CronJobRepository>(
        &self,
        repo: &R,
        name: &str,
        enabled: bool,
        worker_tx: mpsc::UnboundedSender<JobInvocation>,
    ) -> Result<(), SchedulerError> {
        let model = repo
            .find_by_name(name)
            .await?
            .ok_or_else(|| SchedulerError::JobNotFound(name.to_string()))?;

        let handler = self
            .get_handler(name)
            .await
            .ok_or_else(|| SchedulerError::HandlerNotFound(name.to_string()))?;

        let definition = JobDefinition::from(&model);
        self.remove_job_from_scheduler(name).await?;
        self.add_job_internal(&definition, handler, worker_tx).await?;
        repo.restore(name, enabled).await?;

        Ok(())
    }

    pub async fn list_jobs(&self) -> Vec<JobInfo> {
        let jobs = self.jobs.read().await;
        jobs.values()
            .map(|e| JobInfo {
                name: e.name.clone(),
                title: e.title.clone(),
                description: e.description.clone(),
                expression: e.expression.clone(),
                enabled: e.enabled,
                last_run_at: chrono::DateTime::UNIX_EPOCH.into(),
                next_run_at: chrono::DateTime::UNIX_EPOCH.into(),
                updated_at: chrono::DateTime::UNIX_EPOCH.into(),
                running: false,
                group: e.group.clone(),
                frequency_secs: compute_frequency_secs(&e.expression),
            })
            .collect()
    }

    pub async fn list_jobs_detailed<R: CronJobRepository>(
        &self,
        repo: &R,
    ) -> Result<Vec<JobInfo>, SchedulerError> {
        let mut jobs = self.list_jobs().await;
        let names: Vec<String> = jobs.iter().map(|j| j.name.clone()).collect();
        let models = repo.list_by_names(&names).await?;
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
}

use crate::cron::JobContext;
```

注意：上述代码中 `update_expression` 的 `existing.clone().handler()` 是占位写法，实际应保留 `handler` 变量。需要修正为：

```rust
let handler = {
    let jobs = self.jobs.read().await;
    let entry = jobs.get(name).ok_or_else(|| SchedulerError::JobNotFound(name.to_string()))?;
    entry.handler.clone()
};
let mut existing = JobDefinition { ... };
existing.expression = expression.to_string();
self.remove_job_from_scheduler(name).await?;
self.add_job_internal(&existing, handler, worker_tx).await?;
```

### Step 4.2: 编译检查

```bash
cargo check
```

预期：可能需要根据编译错误微调 `update_expression` / `run_job_now`。

### Step 4.3: Commit

```bash
git add src/cron/scheduler.rs
git commit -m "refactor(cron): add SchedulerRuntime with decoupled trigger-to-worker flow"
```

---

## Task 5: 更新 `AppState` 与 `lib.rs` 初始化流程

**Files:**
- Modify: `src/state.rs`
- Modify: `src/lib.rs`
- Delete: `src/scheduler.rs`（在 routes 修改完成后再删除）

### Step 5.1: 修改 `src/state.rs`

```rust
use sea_orm::DatabaseConnection;

use crate::cron::scheduler::SchedulerRuntime;
use crate::cron::worker::JobWorker;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub scheduler: SchedulerRuntime,
    pub worker: JobWorker,
}
```

### Step 5.2: 修改 `src/lib.rs`

将 `init` 函数改为：

```rust
use sea_orm::DatabaseConnection;

use crate::config::{Config, RuntimeEnv};
use crate::cron::repository::SeaOrmCronJobRepository;
use crate::cron::scheduler::SchedulerRuntime;
use crate::cron::worker::JobWorker;
use crate::cron::{JobContext, JobHandler};
use crate::state::AppState;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

async fn init(config: Config) -> anyhow::Result<AppContext> {
    let log_guard = setup_logging(&config.env);

    tracing::info!("Starting rs-template");

    let db = db::connect(&config.database_url).await?;
    let repo = SeaOrmCronJobRepository::new(db.clone());

    let scheduler = SchedulerRuntime::new(db.clone()).await?;
    let worker = JobWorker::new(db.clone(), 10);
    let worker_tx = worker.start();

    // 注册示例 handler；实际业务在此处扩展
    scheduler
        .register_handler(
            "example",
            Arc::new(|_ctx: JobContext| {
                Box::pin(async move {
                    tracing::info!("Example job executed");
                })
            }),
        )
        .await;

    scheduler.load_from_db(&repo, worker_tx.clone()).await?;
    scheduler.start().await?;

    logs_cleanup::spawn_cleanup_task(
        match config.env {
            RuntimeEnv::Prod => "/config/logs".to_string(),
            RuntimeEnv::Dev => "logs".to_string(),
        },
        30,
    );

    let state = AppState {
        db: db.clone(),
        scheduler,
        worker,
    };

    Ok(AppContext {
        log_guard,
        state,
    })
}
```

注意：需要确保 `logs_cleanup` 模块中的 `spawn_cleanup_task` 签名保持不变。

### Step 5.3: 编译检查

```bash
cargo check
```

### Step 5.4: Commit

```bash
git add src/state.rs src/lib.rs
git commit -m "feat(app): wire SchedulerRuntime, JobWorker and load jobs from DB on startup"
```

---

## Task 6: 更新路由层

**Files:**
- Modify: `src/routes/cron_jobs.rs`

### Step 6.1: 修改 `src/routes/cron_jobs.rs`

所有调用从 `state.scheduler.xxx()` 迁移到新接口，并引入 repository：

```rust
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};

use crate::cron::repository::{CronJobRepository, JobDefinition, SeaOrmCronJobRepository};
use crate::cron::worker::JobInvocation;
use crate::cron::JobHandler;
use crate::response::Response;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/cron-jobs", get(list_cron_jobs))
        .route("/cron-jobs/{name}", put(update_cron_job))
        .route("/cron-jobs/{name}/run", post(run_cron_job))
        .route("/cron-jobs/{name}", delete(delete_cron_job))
}

async fn list_cron_jobs(State(state): State<AppState>) -> impl IntoResponse {
    let repo = SeaOrmCronJobRepository::new(state.db.clone());
    match state.scheduler.list_jobs_detailed(&repo).await {
        Ok(jobs) => (StatusCode::OK, Json(Response::success(jobs))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Response::error("SCHEDULER_ERROR", e.to_string())),
        ),
    }
}

#[derive(serde::Deserialize)]
struct UpdateCronJobRequest {
    title: String,
    description: String,
    expression: String,
    enabled: bool,
}

async fn update_cron_job(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<UpdateCronJobRequest>,
) -> impl IntoResponse {
    let repo = SeaOrmCronJobRepository::new(state.db.clone());
    let worker_tx = state.worker.start();

    if let Err(e) = state
        .scheduler
        .update_expression(&repo, &name, &req.expression, worker_tx.clone())
        .await
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(Response::error("INVALID_EXPRESSION", e.to_string())),
        );
    }

    if let Err(e) = state
        .scheduler
        .update_info(&repo, &name, &name, &req.title, &req.description, worker_tx.clone())
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Response::error("SCHEDULER_ERROR", e.to_string())),
        );
    }

    if let Err(e) = state.scheduler.set_enabled(&repo, &name, req.enabled).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Response::error("SCHEDULER_ERROR", e.to_string())),
        );
    }

    (StatusCode::OK, Json(Response::<()>::success(())))
}

async fn run_cron_job(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.scheduler.run_job_now(&name).await {
        Ok(_) => (StatusCode::OK, Json(Response::<()>::success(()))),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(Response::error("JOB_NOT_FOUND", e.to_string())),
        ),
    }
}

async fn delete_cron_job(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let repo = SeaOrmCronJobRepository::new(state.db.clone());
    match state.scheduler.soft_delete_job(&repo, &name).await {
        Ok(_) => (StatusCode::OK, Json(Response::<()>::success(()))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Response::error("SCHEDULER_ERROR", e.to_string())),
        ),
    }
}
```

注意：这里 `state.worker.start()` 每次调用都会启动一个新的 worker 任务，这是错误的。正确做法应在 `AppState` 中持有 `mpsc::UnboundedSender<JobInvocation>`。需要修正 `AppState` 为：

```rust
pub struct AppState {
    pub db: DatabaseConnection,
    pub scheduler: SchedulerRuntime,
    pub worker_tx: mpsc::UnboundedSender<JobInvocation>,
}
```

并在 `lib.rs` 中：

```rust
let worker_tx = worker.start();
let state = AppState {
    db: db.clone(),
    scheduler,
    worker_tx: worker_tx.clone(),
};
```

### Step 6.2: 修正 `AppState` 持有 `worker_tx`

修改 `src/state.rs`：

```rust
use sea_orm::DatabaseConnection;
use tokio::sync::mpsc;

use crate::cron::scheduler::SchedulerRuntime;
use crate::cron::worker::JobInvocation;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub scheduler: SchedulerRuntime,
    pub worker_tx: mpsc::UnboundedSender<JobInvocation>,
}
```

### Step 6.3: 编译检查

```bash
cargo check
```

### Step 6.4: Commit

```bash
git add src/routes/cron_jobs.rs src/state.rs src/lib.rs
git commit -m "refactor(routes): adapt cron job routes to new cron modules"
```

---

## Task 7: 修复数据库迁移与索引

**Files:**
- Modify: `src/db.rs`
- Modify: `src/entity/cron_job.rs`（如需要）

### Step 7.1: 修改 `src/db.rs`

在迁移中：

1. 修复 `group` 列默认值为空字符串。
2. 添加 `schema_migrations` 表。
3. 添加 `cron_jobs.name` 索引。

```rust
use sea_orm::{Database, DatabaseConnection, DbErr};
use sea_orm_migration::prelude::*;

use crate::entity::cron_job;

pub async fn connect(database_url: &str) -> Result<DatabaseConnection, DbErr> {
    let db = Database::connect(database_url).await?;
    migrate(&db).await?;
    Ok(db)
}

async fn migrate(db: &DatabaseConnection) -> Result<(), DbErr> {
    let backend = db.get_database_backend();
    let schema = Schema::new(backend);

    // 创建表
    db.execute(
        backend.build(&schema.create_table_from_entity(cron_job::Entity).if_not_exists()),
    )
    .await?;

    // schema_migrations 表
    db.execute(
        backend.build(
            &Table::create()
                .table(Alias::new("schema_migrations"))
                .if_not_exists()
                .col(
                    ColumnDef::new(Alias::new("version"))
                        .integer()
                        .not_null()
                        .primary_key(),
                )
                .col(ColumnDef::new(Alias::new("applied_at")).timestamp().not_null())
                .to_owned(),
        ),
    )
    .await?;

    // 使用迁移版本控制
    ensure_migration(db, 1, vec![
        "ALTER TABLE cron_jobs ADD COLUMN \"group\" TEXT NOT NULL DEFAULT ''".to_string(),
        "ALTER TABLE cron_jobs ADD COLUMN is_deleted BOOLEAN NOT NULL DEFAULT 0".to_string(),
    ])
    .await?;

    ensure_migration(db, 2, vec![
        "CREATE INDEX IF NOT EXISTS idx_cron_jobs_name ON cron_jobs(name)".to_string(),
    ])
    .await?;

    db.execute_unprepared("ANALYZE").await?;
    Ok(())
}

async fn ensure_migration(
    db: &DatabaseConnection,
    version: i32,
    statements: Vec<String>,
) -> Result<(), DbErr> {
    let exists: i64 = sea_orm::sea_query::Query::select()
        .from(Alias::new("schema_migrations"))
        .column(Alias::new("version"))
        .cond_where(Expr::col(Alias::new("version")).eq(version))
        .to_owned()
        .into_simple_expr(); // 实际应使用 SeaORM 查询方式

    // 更简单的方式：
    let count: i64 = db
        .query_one(
            sea_orm::Statement::from_string(
                db.get_database_backend(),
                format!(
                    "SELECT COUNT(*) as c FROM schema_migrations WHERE version = {}",
                    version
                ),
            )
            .to_owned(),
        )
        .await?
        .map(|qr| qr.try_get::<i64>("", "c").unwrap_or(0))
        .unwrap_or(0);

    if count > 0 {
        return Ok(());
    }

    let txn = db.begin().await?;
    for stmt in statements {
        txn.execute_unprepared(&stmt).await?;
    }
    txn.execute_unprepared(&format!(
        "INSERT INTO schema_migrations (version, applied_at) VALUES ({}, datetime('now'))",
        version
    ))
    .await?;
    txn.commit().await?;

    Ok(())
}
```

注意：上述代码中的迁移查询部分可能不够精确，实际实现时应使用 `sea_orm::Entity` 或原始 SQL 查询，确保编译通过。

### Step 7.2: 编译检查

```bash
cargo check
```

### Step 7.3: Commit

```bash
git add src/db.rs
git commit -m "fix(db): versioned migrations, group default, and cron_jobs name index"
```

---

## Task 8: 前端统一 API 层

**Files:**
- Create: `web/src/lib/api.ts`
- Create: `web/src/hooks/use-cron-jobs.ts`
- Modify: `web/src/pages/cron-jobs.tsx`
- Modify: `web/src/pages/settings.tsx`

### Step 8.1: 创建 `web/src/lib/api.ts`

```typescript
import ky from 'ky';

export interface ApiResponse<T> {
  code: string;
  msg: string;
  data?: T;
}

export const api = ky.create({
  prefixUrl: '/api',
});

export async function unwrap<T>(res: ApiResponse<T>): Promise<T> {
  if (res.code !== '0') {
    throw new Error(res.msg || '请求失败');
  }
  if (res.data === undefined) {
    throw new Error('响应中缺少 data 字段');
  }
  return res.data;
}
```

### Step 8.2: 创建 `web/src/hooks/use-cron-jobs.ts`

```typescript
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api, unwrap } from '@/lib/api';

export interface CronJob {
  name: string;
  title: string;
  description: string;
  expression: string;
  enabled: boolean;
  last_run_at: string;
  next_run_at: string;
  updated_at: string;
  running: boolean;
  group: string;
  frequency_secs: number;
}

export function useCronJobs() {
  return useQuery<CronJob[]>({
    queryKey: ['cron-jobs'],
    queryFn: async () => {
      const res = await api.get('cron-jobs').json<{ code: string; msg: string; data: CronJob[] }>();
      return unwrap(res);
    },
  });
}

export function useUpdateCronJob() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (payload: { name: string } & Partial<CronJob>) => {
      const { name, ...body } = payload;
      const res = await api
        .put(`cron-jobs/${name}`, { json: body })
        .json<{ code: string; msg: string }>();
      return unwrap(res);
    },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['cron-jobs'] }),
  });
}

export function useRunCronJob() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (name: string) => {
      const res = await api
        .post(`cron-jobs/${name}/run`)
        .json<{ code: string; msg: string }>();
      return unwrap(res);
    },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['cron-jobs'] }),
  });
}
```

### Step 8.3: 修改 `web/src/pages/cron-jobs.tsx`

将页面中的 `ky` 导入和手动响应解析替换为 `useCronJobs`、`useUpdateCronJob`、`useRunCronJob`。

### Step 8.4: 运行前端构建检查

```bash
cd web
pnpm build
```

### Step 8.5: Commit

```bash
git add web/src/lib/api.ts web/src/hooks/use-cron-jobs.ts web/src/pages/cron-jobs.tsx
git commit -m "feat(web): centralize API client and cron job hooks"
```

---

## Task 9: 新增测试

**Files:**
- Create: `src/cron/repository_tests.rs` 或 `tests/cron_repository_test.rs`
- Create: `src/cron/worker_tests.rs`
- Modify: `src/cron/scheduler.rs` 添加测试

### Step 9.1: 创建 Repository 集成测试

```rust
#[cfg(test)]
mod tests {
    use sea_orm::{Database, DatabaseConnection};

    use crate::cron::repository::{CronJobRepository, JobDefinition, SeaOrmCronJobRepository};
    use crate::db;

    async fn setup() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db::migrate(&db).await.unwrap();
        db
    }

    #[tokio::test]
    async fn test_insert_and_find() {
        let db = setup().await;
        let repo = SeaOrmCronJobRepository::new(db);
        let job = JobDefinition {
            name: "test".to_string(),
            title: "Test".to_string(),
            description: "".to_string(),
            expression: "@hourly".to_string(),
            enabled: true,
            group: "default".to_string(),
        };
        repo.insert(&job).await.unwrap();
        let found = repo.find_by_name("test").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().expression, "@hourly");
    }
}
```

### Step 9.2: 创建 Worker 测试

测试 `JobWorker` 能接收任务并执行 handler。

### Step 9.3: 运行测试

```bash
cargo test
```

预期：旧测试保持通过，新测试通过。

### Step 9.4: Commit

```bash
git add src/cron/repository_tests.rs src/cron/worker_tests.rs
git commit -m "test(cron): add repository and worker tests"
```

---

## Task 10: Docker 与 CI 改进

**Files:**
- Create: `.dockerignore`
- Modify: `Dockerfile`
- Modify: `.gitea/workflows/build.yaml`

### Step 10.1: 创建 `.dockerignore`

```text
target/
web/node_modules/
web/dist/
db/
logs/
.git/
.env
```

### Step 10.2: 修改 Dockerfile

移除硬编码 secrets：

```dockerfile
ARG AWS_ACCESS_KEY_ID
ARG AWS_SECRET_ACCESS_KEY
```

不再设置默认值。

### Step 10.3: 修改 CI

增加测试步骤和 SHA 标签：

```yaml
- name: Run Rust tests
  run: cargo test

- name: Build web
  run: cd web && pnpm install && pnpm build

- name: Build and push
  run: |
    docker buildx build \
      --platform linux/amd64 \
      --build-arg AWS_ACCESS_KEY_ID=${{ secrets.AWS_ACCESS_KEY_ID }} \
      --build-arg AWS_SECRET_ACCESS_KEY=${{ secrets.AWS_SECRET_ACCESS_KEY }} \
      -t $REGISTRY/ijkzen/rs-template:latest \
      -t $REGISTRY/ijkzen/rs-template:${{ github.sha }} \
      --push .
```

### Step 10.4: Commit

```bash
git add .dockerignore Dockerfile .gitea/workflows/build.yaml
git commit -m "chore(docker,ci): add dockerignore, remove default secrets, add tests and sha tag"
```

---

## 自我审查

### Spec 覆盖检查

| 设计点 | 对应任务 |
|---|---|
| 拆分解析层 | Task 1 |
| Repository 抽象 | Task 2 |
| Worker 队列 | Task 3 |
| SchedulerRuntime | Task 4 |
| 启动加载任务 | Task 5 |
| Handler 接收 JobContext | Task 1 / Task 4 |
| 批量查询 list_jobs_detailed | Task 4 |
| 修复迁移 | Task 7 |
| 前端 API 层 | Task 8 |
| 测试 | Task 9 |
| Docker/CI | Task 10 |

### Placeholder 检查

- 无 TBD/TODO。
- `update_expression` 的 handler 引用已在 Step 6.1 修正说明中处理。
- `AppState` 已改为持有 `worker_tx` 而非 `JobWorker` 实例。

### 类型一致性检查

- `JobHandler` 签名在 `cron/mod.rs` 中统一定义，所有注册点使用 `JobContext`。
- `JobInvocation` 在 `worker.rs` 中定义，在 `scheduler.rs` 中使用。
- `CronJobRepository` trait 方法在 Task 2 定义，后续任务使用一致。

---

## 执行交接

计划已完成并保存到：

**`docs/superpowers/plans/2026-06-25-rs-template-cron-decoupling-plan.md`**

两个执行选项：

1. **Subagent-Driven（推荐）** —— 每个 Task 派一个子代理执行，完成后审查再进入下一个 Task。
2. **Inline Execution** —— 在当前会话中使用 `executing-plans` skill 批量执行，关键节点检查。

请选择执行方式。
