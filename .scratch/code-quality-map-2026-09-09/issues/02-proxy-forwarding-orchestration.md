# 02 · proxy 转发编排与选路审查

Type: task
Status: open
Blocked by: 01

## Question

对 proxy 转发编排与选路域做全量审查：`lb.rs` / `route.rs` / `calls.rs` / `headers.rs` / `forward.rs`（含 forward_chat_direct）/ `failover.rs` / `native.rs` / `usage_rank.rs` 及域内测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：空候选守卫、failover 边界、选路/排序口径、头组装顺序、并发竞态等潜在 bug；
- 实现简洁：重复分支、可收敛拷贝（2026-09-08 已收口成员循环/路由解析/统一流泵的**残余**，不复查已整改项本身）；
- 测试覆盖：哪些分支无测试锁定（对照 48 场景矩阵等已有测试）；
- 模块间调用：与 usage 缓存/额度门控、availability 谓词、cron failure_recovery 探活、metrics 落库的调用是否合适。

产出 `.scratch/code-quality-map-2026-09-09/findings/02-proxy-forwarding-orchestration.md`，Answer 给摘要与需拍板问题。
