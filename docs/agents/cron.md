# 定时任务子系统

分层与运行时契约的权威记录见 ADR-0019（计划回写见 ADR-0007）。代码在 `src/cron/`。

## 三层协作

- **SchedulerRuntime**（`src/cron/scheduler.rs`）：管理任务生命周期。启动时从 DB 加载 `is_deleted = false` 的任务，但**要求先在代码中注册对应的 Handler**，否则任务会被跳过（跳过即不出现在列表 API 中，也无法通过 API 操作）。
- **JobWorker**（`src/cron/worker.rs`）：有界 mpsc 队列 + 信号量并发池，负责真正执行 Handler、记录执行日志并回写 `last_run_at`/`next_run_at`。
- **croner**（`src/cron/parser.rs`）：解析表达式、计算下次运行时间与执行频率。

## Handler 注册与内置任务

业务 Handler 在 `src/lib.rs::init()` 中通过 `scheduler.register_handler(name, handler)` 注册。当前注册：

| Handler | 调度 | 职责 |
| --- | --- | --- |
| `usage_refresh` | `@every 5m` | 刷新全部用量供应商并落库、订阅额度门控与边界实测探活（`src/usage/persist.rs`） |
| `failure_recovery` | `@hourly` | 恢复连续失败禁用供应商 |
| `stats_snapshot` | `@every 1h` | 统计快照生成（ADR-0021） |
| `stats_snapshot_rebuild` | `@every 1h` | 统计快照自愈补缺 |
| `models_refresh` | `0 0 4 * * *` | 从 models.dev 拉取模型目录并原子写外置文件（ADR-0025） |

**新增内置任务 = 种子行 + Handler 注册一一对应**，缺一即不出现、不执行：内置周期任务在启动时经 `src/cron/seed.rs` 幂等插入种子行。没有创建任务的 API。

Handler 类型：

```rust
pub type JobHandler = Arc<
    dyn Fn(JobContext) -> Pin<Box<dyn Future<Output = Result<(), JobError>> + Send>> + Send + Sync,
>;
```

`JobContext` 目前只携带 `db: DatabaseConnection`。

## 执行日志

worker 执行 handler 时创建带 `job_name`/`run_id` 字段的 span，`JobLogLayer` 捕获 span 内的 tracing 日志（级别受 `RUST_LOG` 限制，默认 info 起步）后经 broadcast（容量 8192）分发：worker 落库（`cron_job_runs` + `cron_job_logs`），SSE 端点实时推送。

每次执行算一个 run，任务最多保留最近 30 次执行（更早的连同日志清理），单次执行最多 2000 条日志（超出丢弃并标记截断）。handler 失败/panic 会追加一条 `任务执行失败：...` 系统日志；进程重启时残留的 running run 会被标记为 failed。手动「立即执行」与调度触发走同一通道，同样记录日志。

日志 API：

- `GET /api/cron-jobs/{name}/logs`：最近 30 次执行列表。
- `GET /api/cron-jobs/{name}/logs/{run_id}`：某次执行的全部日志，不存在返回 404。
- `GET /api/cron-jobs/{name}/logs/stream`：SSE。连接时若任务在执行中先发 `snapshot` 回放已落库日志，否则发 `idle`；后续推送 `log`/`run_started`/`run_ended`；接收端积压时发 `reset` 让前端重拉。

## 关键行为约定

- **禁用 = 从调度器移除**：tokio-cron-scheduler 的 `set_stop()` 在其内存存储实现下不会阻止任务触发，因此禁用任务是将 job 从 `JobScheduler` 中移除、但保留在内存列表中（仍可查看与手动「立即执行」）；启用时重新创建 job。
- **时区**：Cron 表达式按**服务器本地时区**解释（`0 0 8 * * *` = 本地时间每天 8 点），与前端浏览器本地时间展示一致。部署容器（根 `Dockerfile` 与 `deploy/Dockerfile`，均安装 tzdata）设置 `TZ=Asia/Shanghai`，compose 文件亦有 `TZ: Asia/Shanghai` 兜底，因此生产环境 cron 语义为东八区。tokio-cron-scheduler 在创建 job 时快照 UTC 偏移，有夏令时的地区跨切换点需重建 job 才用新偏移；东八区无夏令时，不受影响。
- **支持的表达式**：
  - 标准 Cron：5 字段（自动补秒为 `0`）或 6 字段（秒 分 时 日 月 周）。
  - 便捷宏：`@yearly`、`@monthly`、`@weekly`、`@daily`、`@hourly`。
  - 间隔语法：`@every 5m`、`@every 1h30m` 等（单位 `s/m/h/d`，可组合）。
- **错过执行**：重启时已过期的 cron 任务不会补跑，`next_run_at` 会被重算到下一次；`@every` 任务重启后从当前时间重新计间隔。
- **优雅关闭**：收到 SIGINT/SIGTERM 后，先停 HTTP 服务，再停调度器（不再派发新任务），最后等待在跑任务结束（10 秒超时后放弃并退出）。
- **API 面**：只支持更新、立即执行、软删除已有任务，没有创建任务的接口（`SchedulerRuntime::add_job` 可供代码内使用）。对未加载进调度器（无 handler）的任务调用更新接口直接返回 400，不会修改 DB。

## 测试注意

依赖全局 tracing subscriber 的测试（`log_capture` 与 worker 日志链路）通过 `SUBSCRIBER_LOCK` 串行执行；worker 日志测试需用 `current_thread` runtime（`set_default` 是线程局部的）。调度器行为回归：禁用的任务不会触发（`set_stop` 在 tokio-cron-scheduler 内存存储下无效，禁用必须走移除）、启用后恢复触发、禁用任务仍可手动执行。
