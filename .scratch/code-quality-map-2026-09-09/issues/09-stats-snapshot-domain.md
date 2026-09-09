# 09 · stats_snapshot 快照域审查

Type: task
Status: open
Blocked by: 01

## Question

对统计快照域做全量审查：`stats_snapshot/` 全目录（core 桶帧/闭桶判定/窗口分解、registry 指标主体注册表、generator GROUPING SETS 生成、reader 兑底、subject 键解析、tasks 回填/自愈）+ 域内测试（含等价测试）。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：闭桶判定/哨兵行/分位标量口径、水位推进、时区重算、删主体降级（09-09 已修项不复查）、517 竞态修复（事务首写）后有无同型残余；
- 实现简洁：registry 谓词单一事实源是否被读侧绕行、生成器与读侧重复；
- 测试覆盖：generator_tests 之外缺什么（自愈补缺、回填幂等边界）；
- 模块间调用：与 10 读端点、db 迁移、cron tasks 的双向关系是否合适。

产出 `.scratch/code-quality-map-2026-09-09/findings/09-stats-snapshot-domain.md`，Answer 给摘要与需拍板问题。
