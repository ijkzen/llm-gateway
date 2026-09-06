# 01: 迁移 21：新增 disabled_reason 列并回填

**What to build:** 旧结构数据库升级后，provider 表获得机器可读的「停用原因」字段：启用行回填 `None`、连续失败禁用行回填 `failure`、其余禁用行按安全默认回填 `manual`（宁多一次手动启用，不自动启用存量停用行）。`failure_disabled` 旧列本票保留不动（expand 阶段），迁移幂等、可重跑。

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [x] 迁移编号衔接现行最高版本（不撞生产残留的废弃号段守卫）
- [x] 合成旧结构库跑迁移后，三类存量行的回填结果符合规则
- [x] 重复执行迁移幂等，schema_migrations 版本行正确
- [x] 迁移后 `PRAGMA table_info(provider)` 可见新列（作为测试断言或验证说明）

## Comments

- 79b8aaa（feat/availability-state-machine）实施完成；全量质量门绿（fmt/clippy -D warnings/cargo test 634 通过；前端无改动，主仓 lint+vitest 335 通过）。迁移 21（expand）：disabled_reason 列 + 三规则回填；failure_disabled 保留。合成旧库回填测试与幂等测试通过。
