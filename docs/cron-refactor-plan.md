# 定时任务设计缺陷改造计划

> 生成时间：2026-07-01
> 依据：代码走查 + 与用户逐条确认

## 变更清单

| 编号 | 问题 | 决策 | 改动范围 |
|---|---|---|---|
| 5 | `@every` 任务重启后 `next_run_at` 显示不准 | **修改**：加载时重新计算并写回 DB | `src/cron/scheduler.rs` |
| 6 | `group` 字段无法编辑/展示 | **修改**：后端支持更新 group，前端按分组展示并支持滚动 | `src/routes/cron_jobs.rs`, `src/cron/scheduler.rs`, `web/src/pages/cron-jobs.tsx`, `web/src/hooks/use-cron-jobs.ts` |
| 7 | 未执行任务显示成 1970 年 | **修改**：未执行显示“等待执行” | `web/src/pages/cron-jobs.tsx` |
| 9 | `name` 上重复索引 | **修改**：删除冗余索引 | `src/db.rs` |
| 10 | 更新时 DB 与调度器状态不一致 | **修改**：调整执行顺序，调度器操作成功后再写 DB，失败时回滚或不写 | `src/cron/scheduler.rs`, `src/cron/repository.rs` |
| 12 | Worker 无界通道 | **修改**：改为有界通道，队列上限可配置，默认 1000 | `src/cron/worker.rs`, `src/cron/scheduler.rs`, `src/config/mod.rs`, `src/lib.rs` |
| 13 | `@every` 按完成时间推进 | **修改**：按任务触发/计划时间推进 | `src/cron/worker.rs`, `src/cron/scheduler.rs` |
| 15 | 通道错误映射成 HandlerNotFound | **修改**：新增 `WorkerChannelClosed` 错误类型 | `src/cron/mod.rs`, `src/cron/scheduler.rs`, `src/routes/cron_jobs.rs` |
| 16 | 编辑允许空表达式/标题 | **修改**：前后端都加非空校验 | `src/routes/cron_jobs.rs`, `web/src/pages/cron-jobs.tsx` |
| 2 | 未注册 handler 的任务被静默跳过 | **不修改跳过逻辑，仅优化日志**：启动后统一打印被跳过的任务名列表 | `src/cron/scheduler.rs` |
| 1/4/8/11/14/17 | 创建 API、恢复 API、执行日志、set_enabled 行为、解析器增强、认证 | **保持现状** | 无改动 |

## 详细设计

### 5. `@every` 重启后 next_run_at 重置

在 `SchedulerRuntime::load_from_db` 中：
- 对每条 active 任务，若表达式解析为 `ScheduleType::Every(duration)`，则用当前时间重新计算 `next_run_at`。
- 调用 `repo.update_run_times(name, last_run_at, new_next_run_at)` 写回 DB。
- Cron 表达式任务不需要重置，因为 `compute_next_run` 是确定性的。

### 6. group 字段可编辑 + 前端分组展示

后端：
- `UpdateJobRequest` 增加 `group: Option<String>`。
- `SchedulerRuntime::update_info` 签名增加 `group: &str` 参数，更新 `JobDefinition::group`。
- 路由 `update_job` 在请求包含 group 时传给它。

前端：
- `CronJob` 接口已有 group。
- 编辑弹窗增加 group 输入框。
- 列表页按 `group` 分组渲染，每个分组一个子表格/卡片。
- 整个任务列表区域设置 `overflow-auto` + `max-h`，超出屏幕可滚动。

### 7. 未执行任务显示“等待执行”

前端 `formatDate`：
- 判断 `last_run_at` 的时间戳是否 <= 0（epoch）。
- 若是则返回 `"等待执行"`。
- 保持 `next_run_at` 正常格式化显示。

### 9. 删除冗余索引

新增迁移版本 3：
```sql
DROP INDEX IF EXISTS idx_cron_jobs_name
```
因为 `name` 的 `#[sea_orm(unique)]` 已自动创建唯一索引。

### 10. 更新时保证 DB 与调度器一致

策略：**先更新调度器，成功后再写 DB**。
- `update_expression`：解析/校验表达式 → 从调度器移除旧 job → 添加新 job → 成功后更新 DB。
- `update_info`：更新 handler map（若改名）→ 从调度器移除旧 job → 添加新 job → 成功后更新 DB。
- `set_enabled`：先调用 `job.set_stop(!enabled)` → 成功后更新 DB。
- `soft_delete_job`：先从调度器移除 → 成功后更新 DB。
- `restore_job`：先 add_job_internal → 成功后更新 DB。
- 若调度器操作失败，DB 不会被修改，重启后状态一致。
- 若 DB 更新失败但调度器已改，运行时短暂不一致，下次重启会恢复；这种情况极少且无法完全避免（外部调度器无法参与 DB 事务）。

### 12. Worker 有界通道

- 新增配置项 `CRON_JOB_QUEUE_SIZE`，默认 1000。
- `JobWorker::new` 增加 `queue_size: usize` 参数。
- 使用 `tokio::sync::mpsc::channel` 替代 `unbounded_channel`。
- `run_job_now` 使用 `worker_tx.send(...).await`，通道满时等待。
- 调度器闭包里的 `tx.send(inv).await` 同样等待；若发送失败（通道关闭）则记录 error。

### 13. `@every` 按触发时间推进

- `JobInvocation` 增加 `scheduled_at: DateTime<Local>`。
- 调度器触发时把当前时间作为 `scheduled_at` 传入。
- Worker 执行完 handler 后：
  - Cron 表达式：用 `compute_next_run` 计算下一次。
  - `@every`：`next_run_at = scheduled_at + interval`。
- 这样即使任务执行耗时，间隔也是从上次触发时间开始算。

### 15. WorkerChannelClosed 错误类型

- `SchedulerError` 新增 `WorkerChannelClosed(String)`。
- `run_job_now` 中 `worker_tx.send` 失败时返回该错误。
- 路由错误码映射为 500。

### 16. 编辑非空校验

后端：
- `update_job` 中，若 `title` 或 `expression` 为 `Some("")`，返回 400 `INVALID_INPUT`。

前端：
- 编辑弹窗中 title 或 expression 为空时，禁用保存按钮并显示提示。

### 2. 优化未注册 handler 的日志

- `load_from_db` 收集被跳过的任务名。
- 遍历结束后统一打印一条 warn：
  ```
  Skipped N cron jobs because their handlers are not registered: [name1, name2]
  ```
- 避免每个任务一条日志刷屏。

## 验证项

- `cargo test` 通过。
- `cd web && pnpm build` 通过。
- 手动验证：
  1. 启动后端，创建/编辑 `@every` 任务，重启后 `next_run_at` 被重置。
  2. 编辑 group，前端按新分组展示。
  3. 未执行任务的 `last_run_at` 显示“等待执行”。
  4. 空 title/expression 无法保存。
  5. Worker 队列大小通过环境变量可调。
