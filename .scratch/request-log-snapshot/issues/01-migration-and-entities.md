# 01: 迁移 26 — request_log_snapshot 与 meta 表

**What to build:** 新建 `request_log_snapshot` 窄表（id, duration_type, start_time, end_time, entity_type, entity, metric_type, metric_value，UNIQUE(duration_type, start_time, entity_type, entity, metric_type)）与快照 meta 键值表（存生成时区等），迁移号 26（生产 14/15 号段废弃，勿撞号），建所需索引；提供 SeaORM 实体与 meta 读写辅助。新库/老库迁移均幂等（ANALYZE 惯例）。

**Blocked by:** None（可立即开工）

**Status:** completed (2026-09-09 实现并验证)

- [ ] `migrate()` 新库建齐两表且 schema_migrations 记 26；老库升级幂等
- [ ] UNIQUE 约束生效（同桶同主体同指标重复 upsert 由生成器幂等处理）
- [ ] 索引覆盖读路径主查询（duration_type+start_time、entity 过滤）
