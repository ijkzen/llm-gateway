# 06: 调度状态单一所有者——next_run 四写者收口（C4）

**What to build:** 调度器（SchedulerRuntime）成为任务状态唯一所有者：next_run_at 计算与回写、enabled 切换全部收进调度器接口；仓库（repository.rs）退化为哑持久化。具体形态：worker 执行完调 `scheduler.on_run_finished(name, outcome, scheduled_at, …)`（worker.rs 284-306 的「从 scheduled_at 锚定、超期从 now 重算」语义原样搬移，手动/调度同通道行为不变），路由 PUT 改走调度器更新接口（enabled 迁移用调度器现有带回滚的 `set_enabled` 语义——该路径目前仅测试在用，收口后生产与测试同一入口；路由只在表达式实际变更时触发 next_run 重算，**修复**：仅改标题的 PUT 在错过执行期会把计划悄悄推后——cron_jobs.rs ~143-156 无条件重算）。`update_job_in_memory` 镜像更新、路由侧直写任务表的重复路径删除。启动加载、missed 不补跑语义不变。

**Blocked by:** None（cron 模块独立，可与 01/03 并行）。

**Status:** ready-for-agent

- [ ] 仅改标题/描述的 PUT 不再触碰 next_run_at（回归测试：把 next_run 置于过去后做 title-only PUT，next_run 不再前移）
- [ ] 表达式变更的 PUT、启停切换、立即执行、调度触发的 next_run/last_run 行为与现状逐项一致（worker.rs 既有 scheduled_at+5m 断言族迁移到新入口）
- [ ] 生产与测试走同一状态迁移入口（set_enabled 测试专用路径消失或合一）
- [ ] scheduler 单测（scheduler/tests.rs 765 行）适配后全绿；worker 日志链路测试不受影响
- [ ] 全量质量门绿

## Comments
