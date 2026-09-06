# 04: 迁移 22：删除 failure_disabled 旧列（contract）

**What to build:** 旧布尔标志列从 provider 表与实体定义中消失，`disabled_reason` 成为停用语义的唯一载体；历史脏数据隐患（旧列与新列不一致的可能）随之根除。部署说明注明需用 `PRAGMA` 实证列变更生效。

**Blocked by:** 03。

**Status:** ready-for-agent

- [ ] 迁移执行后旧列不存在、实体不再定义该字段
- [ ] 全库无任何对旧列的读写残留（编译期即可证明）
- [ ] 全量质量门绿；迁移幂等
