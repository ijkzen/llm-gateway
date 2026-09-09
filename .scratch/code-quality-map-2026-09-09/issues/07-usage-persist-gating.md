# 07 · usage 持久化与额度门控审查

Type: task
Status: open
Blocked by: 01

## Question

对 usage 持久化与门控域做全量审查：`types.rs` / `persist.rs`（缓存写读/全量刷新/apply_usage_gate/probe_boundary_providers/usage_refresh handler）/ `mem_cache.rs` 及域内测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：10 分钟新鲜度单一判定是否真单一、原子 upsert 边界、单飞去重、停用/恢复级联、边界实测探活（09-08 E7 已修项不复查）；
- 实现简洁：判定谓词是否有第二份拷贝、缓存双写面；
- 测试覆盖：quota_gate/boundary_probe 集成之外缺什么（并发刷新、缓存过期竞态单测）；
- 模块间调用：与 06 抓取层、proxy usage_rank、cron seed、availability 停用域的口径是否一致。

产出 `.scratch/code-quality-map-2026-09-09/findings/07-usage-persist-gating.md`，Answer 给摘要与需拍板问题。
