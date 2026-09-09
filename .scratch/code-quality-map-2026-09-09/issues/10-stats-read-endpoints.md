# 10 · stats 读端点审查

Type: task
Status: open
Blocked by: 01

## Question

对统计读端点域做全量审查：`routes/stats.rs` 门面 + `routes/stats/` 子目录（window/compute/summary_charts/insight/rank_impl/metrics）及域内测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：窗口契约三态解析、时区口径（设置表统一后有无残余）、分桶补零、Top-N 折叠边界、兑底路径与快照路径数字一致性（等价测试已锁的不复查）；
- 实现简洁：五 rank handler 是否仍抄写、insight 单 handler 体量、SQL 手拼面；
- 测试覆盖：rank/metrics/insight 单测之外缺什么；
- 模块间调用：与 09 快照域、request 表直读、前端图表契约的边界。

**归位遗留项 S5**：同窗 ~12 次聚合合并单 GROUP BY（09-07 起读路径被 registry 单源化/桶迭代收敛重写过，原语境已变）——在此基于新结构给出重估结论。

产出 `.scratch/code-quality-map-2026-09-09/findings/10-stats-read-endpoints.md`，Answer 给摘要与需拍板问题。
