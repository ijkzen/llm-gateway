# 13 · 数据与实体层审查

Type: task
Status: open
Blocked by: 01

## Question

对数据层做全量审查：`db.rs`（1202 行迁移单体：连接配置/WAL/建表/26 版增量迁移/ANALYZE/ensure_sqlite_dir）、`entity/` 全目录及迁移相关测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：迁移幂等/版本守卫/废弃号段坑（16 起编惯例）、WAL 与 busy_timeout 配置、schema 与 entity 漂移面；
- 实现简洁：迁移单体拆分必要性与风险（对照单文件 ≤1000 行约定——db.rs 已超限，评估拆分方案）、连接参数重复；
- 测试覆盖：26 快照两表测试之外缺什么（坏库恢复、并发迁移）；
- 模块间调用：entity 被全仓直用是否形成合理边界、是否有跨实体事务散落在调用方。

产出 `.scratch/code-quality-map-2026-09-09/findings/13-data-entity-layer.md`，Answer 给摘要与需拍板问题。
