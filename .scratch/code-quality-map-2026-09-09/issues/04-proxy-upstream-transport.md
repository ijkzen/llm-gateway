# 04 · proxy 上游传输与探活审查

Type: task
Status: open
Blocked by: 01

## Question

对 proxy 上游传输域做全量审查：`upstream.rs` / `pool.rs` / `probe.rs`（test_model/probe_provider/ProbeFailure）及域内测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：连接池归还/释放竞态、空闲超时、TTFT 起点口径、探活与真实请求的并发关系；
- 实现简洁：客户端构造、TLS 配置克隆、错误映射是否有收敛空间（09-08 审计已整改项不复查）；
- 测试覆盖：upstream_pool 集成测试之外缺什么（超时/中断/坏上游场景）；
- 模块间调用：probe 被 usage 边界探活与 cron failure_recovery 双消费，接口是否合适。

产出 `.scratch/code-quality-map-2026-09-09/findings/04-proxy-upstream-transport.md`，Answer 给摘要与需拍板问题。
