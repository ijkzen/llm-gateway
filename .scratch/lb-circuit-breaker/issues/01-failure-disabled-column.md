# 01: 禁用来源隔离——failure_disabled 落地

**What to build:** 管理员能区分「额度门控禁用」与「连续失败禁用」两种来源：被连续失败禁用的供应商（带 failure_disabled 标记）即使额度恢复，usage_refresh 也不会自动放回它；管理员手动启用供应商时，标记被清除、供应商回到正常状态。Migration 17 给 provider 表加 `failure_disabled` 布尔列（默认 false，含历史库 column_exists 兼容）。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Migration 17 新增 provider.failure_disabled 布尔列，新库/历史库（含废弃号段残留）都能正确建列
- [ ] usage_refresh 的恢复分支跳过 failure_disabled=true 的供应商；禁用分支与用量刷新行为不变（failure_disabled 供应商照常刷新用量）
- [ ] 手动启用供应商时清除 failure_disabled（手动禁用不设置该标记）
- [ ] 集成测试：置 failure_disabled + 禁用后调 refresh_all_usage（额度充足数据）→ enable 仍为 false；手动启用 → 标记清除、enable=true；先例 provider_quota_gate_integration
