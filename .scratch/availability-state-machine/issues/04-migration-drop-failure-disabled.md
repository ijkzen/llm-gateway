# 04: 迁移 22：删除 failure_disabled 旧列（contract）

**What to build:** 旧布尔标志列从 provider 表与实体定义中消失，`disabled_reason` 成为停用语义的唯一载体；历史脏数据隐患（旧列与新列不一致的可能）随之根除。部署说明注明需用 `PRAGMA` 实证列变更生效。

**Blocked by:** 03。

**Status:** ready-for-agent

- [x] 迁移执行后旧列不存在、实体不再定义该字段
- [x] 全库无任何对旧列的读写残留（编译期即可证明）
- [x] 全量质量门绿；迁移幂等

## Comments

- 46b4d73（feat/availability-state-machine）实施完成；全量质量门绿（fmt/clippy -D warnings/cargo test 634 通过；前端无改动，主仓 lint+vitest 335 通过）。迁移 22（contract）：旧列删除、实体字段移除、全部种子行清理；编译期证明无读写残留。schema 校验更新。
