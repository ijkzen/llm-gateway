# 0019 — 定时任务子系统：注册-调度-执行-日志分层与运行时契约

## Status

accepted

## Context

定时任务子系统从搭建起即按「解析/调度/执行/日志」分层，但大量可执行语义是运行时（tokio-cron-scheduler、SQLite、tracing subscriber）行为逼出来的约束（禁用必须走移除、日志捕获依赖 span、残留 run 需启动标记），没有拍板记录且容易在改动时踩坑。本文按现状固化该子系统的分层与不可违背的运行时契约（计划回写部分见 ADR-0007）。

## Decision

1. **分层**：`SchedulerRuntime`（生命周期：加载/启停/增删改）↔ `JobWorker`（有界 mpsc 队列 + 信号量并发池，容量由 `CRON_JOB_QUEUE_SIZE`/`CRON_JOB_MAX_CONCURRENT` 环境变量控制）↔ `croner`（表达式解析与下次运行时间计算）。仓库**没有创建任务的 API**：管理面只有列表/更新/立即执行/软删除；内置任务（`usage_refresh` `@every 5m`、`failure_recovery` `@hourly`）由启动种子幂等 upsert（`cron/seed.rs`）。DB 中存在但没有注册 Handler 的任务启动即跳过、也不出现在列表 API。
2. **禁用 = 从调度器移除**：tokio-cron-scheduler 的 `set_stop()` 在其内存存储实现下不会阻止触发（scheduler.rs 注释明示），因此禁用任务是从调度器移除但保留在内存列表（仍可查看、可手动执行）；启用时重新创建 job。
3. **时间语义**：表达式按**服务器本地时区**解释（生产容器 TZ=Asia/Shanghai，设置表 timezone 变更会重载 cron 任务，见 ADR-0015）；支持标准 5/6 字段、`@daily` 等宏与 `@every s/m/h/d` 组合语法。错过执行不补跑（重启后 next_run 重算到未来；`@every` 从当前时间重新计间隔）。
4. **执行与日志**：手动「立即执行」与调度触发走同一通道；worker 为每次执行建 span（`job_name`/`run_id`），`JobLogLayer` 捕获 span 内 tracing 日志 → broadcast（8192）双写：落库（`cron_job_runs` + `cron_job_logs`）与 SSE 实时推送（连接时快照回放或 idle，积压发 reset）。每任务最多保留最近 30 次执行（`MAX_RUNS_KEPT`，连带清理），单次执行最多 2000 条日志（超出截断并标记）；handler 失败/panic 追加「任务执行失败」系统日志；进程启动把残留 running 标记为 failed。
5. **回写单写者**：计划推进（scheduled_at 锚定、超期从 now 重算、DB 回写）唯一实现在 `scheduler::on_run_finished`；PUT 仅表达式实际变更才重算 next_run_at（ADR-0007）。优雅关闭：停 HTTP → 停调度器（不再派发）→ 等在跑任务结束（10 秒超时放弃）。

## Consequences

- 执行语义（并发池/日志链路）与调度语义（启停/移除/重建）解耦，改动互不牵连。
- 高危踩点已具名：禁用必须移除而非 set_stop；时区一律本地化解释；日志观测依赖 span 归属，脱离任务 span 的日志不会进 run。
- 新内置任务 = seed 行 + handler 注册一一对应，缺一即不出现/不执行。
