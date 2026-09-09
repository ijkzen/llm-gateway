# 08 · cron 调度与日志链路审查

Type: task
Status: claimed
Blocked by: 01

## Question

对 cron 域做全量审查：`scheduler.rs` / `parser.rs` / `repository.rs` / `worker.rs` / `seed.rs` / `log_capture.rs` / `log_repository.rs` + `scheduler/tests.rs`，**另含 lib.rs:196 注册的 FAILURE_RECOVERY_JOB handler 实现体 `src/proxy/failure_recovery.rs` 的归属审查**（01 盘点结论：唯一消费=cron 注册、转发路径零调用，寄居 proxy/ 名不副实，此处定夺归属建议）。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：禁用=移除语义、重启恢复、优雅关闭、日志截断/计数口径（09-08 已修 E1-E8/P1-P4 系列不复查）、next_run_at 写者归属（C4 已收口不复查）；
- 实现简洁：scheduler 回滚路径与生产路径并存是否仍有冗余、仓库层哑化程度；
- 测试覆盖：worker/log_capture 串行锁测试之外缺什么；
- 模块间调用：worker→调度器回调、路由直写面、seed 与 handler 注册的双源一致性。

产出 `.scratch/code-quality-map-2026-09-09/findings/08-cron-domain.md`，Answer 给摘要与需拍板问题。

## Answer

**清单产出**：`findings/08-cron-domain.md`——1 P2（08-01 SSE seq 契约）+ 12 P3 + 1 归属定夺。方法=scheduler/parser/repository/seed/mod 自读 + worker/log_capture/log_repository 后台子代理逐行深读（含前端 use-cron-job-logs.ts 与 CronJobLogsDialog 契约核对）+ 关键断言磁盘抽核（JobLogEvent seq 全 None/run_ended 先于落库/SSE 路由注释/前端去重实现亲验）。09-08 已修 E1-E8/P1-P4 系列核验无回归。

- **08-01（P2，跨栈契约）**：实时 SSE log 事件从不携带 seq（log_capture.rs:151/63/83 全 None），而 E6「先订阅再快照」防重设计与前端 `data.seq <= last.seq` 去重都建立在 seq 上——live 事件 seq undefined 恒不触发去重，重叠窗口日志重复追加 + React key 恒 undefined；结构体文档自认「携带 seq」与实现不符。**已拍板：全链路补 seq**（JobLogLayer 按 per-span 单调计数器在捕获侧分配 seq，广播 FIFO 保序与 DB flush 同源；Lagged 提示行只落 DB 的号段错位对去重规则安全；前端保留 seq 去重补单测）。
- **08-02~08-07（P3）**：run_ended 广播先于 finish_run 落库（订阅窗口丢 run_ended → 「running 永不结束」，交换顺序）；worker 合成消息（截断提示/失败日志）不经 4096 截断与捕获侧口径不一致；log_repository trait insert_log 死面（生产零调用仅测试用）；flush 失败静默丢批无重试（观察级）；run 状态三写者 + 6h 回收对超长 handler 的 failed→success DB 中间态（内置任务不触发）；seed 行 next_run_at=now 非计算值（@hourly 首轮展示偏差）。
- **测试缺口**（08-08/09）：shutdown 超时重启恢复/队列满/Lagged 真实溢出/并发双 run 归属隔离/insert_run 失败注入/顺序竞态六类 + log_repository 并发 prune/finish 覆盖 6h 行/大批次参数三缺口。
- **归属定夺**：failure_recovery.rs 唯一消费=cron 注册、转发路径零调用，寄居 proxy/ 名不副实。**已拍板：移顶层 src/failure_recovery.rs**（与 availability.rs 平级；依赖 proxy::test_model 已 pub 无环；实施批=移文件 + lib.rs/tests import + AGENTS 树）。

**需拍板问题**：已全部当场拍板（08-01 全链路补 seq、归属移顶层），无遗留。

Status: resolved
