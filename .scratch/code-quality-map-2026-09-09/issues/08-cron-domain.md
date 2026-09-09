# 08 · cron 调度与日志链路审查

Type: task
Status: open
Blocked by: 01

## Question

对 cron 域做全量审查：`scheduler.rs` / `parser.rs` / `repository.rs` / `worker.rs` / `seed.rs` / `log_capture.rs` / `log_repository.rs` + `scheduler/tests.rs`。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：禁用=移除语义、重启恢复、优雅关闭、日志截断/计数口径（09-08 已修 E1-E8/P1-P4 系列不复查）、next_run_at 写者归属（C4 已收口不复查）；
- 实现简洁：scheduler 回滚路径与生产路径并存是否仍有冗余、仓库层哑化程度；
- 测试覆盖：worker/log_capture 串行锁测试之外缺什么；
- 模块间调用：worker→调度器回调、路由直写面、seed 与 handler 注册的双源一致性。

产出 `.scratch/code-quality-map-2026-09-09/findings/08-cron-domain.md`，Answer 给摘要与需拍板问题。
