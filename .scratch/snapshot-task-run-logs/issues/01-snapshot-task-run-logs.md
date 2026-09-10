# 01: 快照任务开始/结束日志（带数据）+ 失败交回 worker

**What to build:** `src/stats_snapshot/tasks.rs` 两个入口（`run_snapshot_generation` / `run_snapshot_heal`）
每次实际执行打开始一行 + 结束一行；结束行带各粒度固化桶数与耗时，回填/时区重算场景额外带覆盖时间范围；
空转照打。`src/lib.rs` 两个 handler 把错误交回 worker（run 记 failed + 自动追加「任务执行失败：…」）。
配套：`tasks.rs` 单测用 tracing 捕获层锁定「有开始/结束且带数据（含空转）」。

**Blocked by:** None（可立即开工）

**Status:** done (2026-09-10)

- [x] 生成任务：开始行 + 增量/回填/时区重算三种结束行（桶数 + 耗时 [+ 覆盖范围]）
- [x] 自愈任务：开始行 + 结束行（检查闭桶 N 个、补算 M 个、耗时），未初始化时有说明行
- [x] 空转（0 个桶 / 补算 0 个）也留下结束行
- [x] 两个 handler 错误交回 worker（run=failed），移除 handler 内自吞错误的写法
- [x] 单测锁定日志文本（tasks.rs 既有测试缝）+ 集成用例锁定 run=failed（tests/stats_snapshot_jobs_integration.rs）
- [x] 全量质量门全绿（cargo fmt / clippy -D warnings / cargo test --all-targets；前端 lint + vitest 复查亦绿）
- [x] AGENTS.md 结构清单与测试计数同步（jobs.rs 模块 + 32 个顶层测试文件 + 936/563/373）
