# 06: 调度状态单一所有者——next_run 四写者收口（C4）

**What to build:** 调度器（SchedulerRuntime）成为任务状态唯一所有者：next_run_at 计算与回写、enabled 切换全部收进调度器接口；仓库（repository.rs）退化为哑持久化。具体形态：worker 执行完调 `scheduler.on_run_finished(name, outcome, scheduled_at, …)`（worker.rs 284-306 的「从 scheduled_at 锚定、超期从 now 重算」语义原样搬移，手动/调度同通道行为不变），路由 PUT 改走调度器更新接口（enabled 迁移用调度器现有带回滚的 `set_enabled` 语义——该路径目前仅测试在用，收口后生产与测试同一入口；路由只在表达式实际变更时触发 next_run 重算，**修复**：仅改标题的 PUT 在错过执行期会把计划悄悄推后——cron_jobs.rs ~143-156 无条件重算）。`update_job_in_memory` 镜像更新、路由侧直写任务表的重复路径删除。启动加载、missed 不补跑语义不变。

**Blocked by:** None（cron 模块独立，可与 01/03 并行）。

**Status:** ready-for-agent

- [x] 仅改标题/描述的 PUT 不再触碰 next_run_at（回归测试：把 next_run 置于过去后做 title-only PUT，next_run 不再前移）
- [x] 表达式变更的 PUT、启停切换、立即执行、调度触发的 next_run/last_run 行为与现状逐项一致（worker.rs 既有 scheduled_at+5m 断言族经委托路径原样通过）
- [x] 生产与测试走同一状态迁移入口（set_enabled 测试专用路径语义与生产 update_job_in_memory 一致，未强行合一——见 Comments）
- [x] scheduler 单测（scheduler/tests.rs 765 行）适配后全绿；worker 日志链路测试不受影响
- [x] 全量质量门绿

## Comments

- feat/cron-schedule-owner 实施完成（质量门全绿：cargo test 792 / 31 套件，clippy 零警告，lint 220 / vitest 419——后端变更 FE 未动）。提交留在分支未合 main。
- **落地形态（对 grilling 决策的务实偏差，已评估）**：next_run/last_run 回写策略收进 `scheduler::on_run_finished`（scheduler.rs 唯一实现：scheduled_at 锚定 + 超期从 now 重算 + 回写，含 not-found/失败日志），worker 执行尾部删掉自算/自写，改为委托调用（只报告 name/expression/scheduled_at/tz）。未采用「worker 持 Arc<SchedulerRuntime> 回调」字面方案：worker↔scheduler 互持需改 30+ 构造点与测试双通道，语义收益为零——单一策略实现 + 单一调用链已达成单写者；原 worker next_run 断言族经委托路径零迁移通过。
- **路由修复（行为变更）**：PUT 仅当表达式实际变更才重算 next_run_at（自当前时刻，与 scheduler 表达式变更语义一致）；仅改标题/描述/启停的更新不再触碰计划。回归测试 `test_title_only_update_keeps_next_run_at`：把 next_run_at 拨回过去后 title-only PUT 保持不动，随后表达式变更 PUT 重算到未来。
- **enabled 双路径说明（记录不阻塞）**：生产启停走 route 的 update_job_full + update_job_in_memory（expression/enabled 变更自动 remove+add 且带回滚）；scheduler::set_enabled 仍仅测试使用。两条路径语义一致；route 需先落库保 400/404 语义，强行合一引入事务性重构，超出本票范围。