# 02 — 定时任务 handler 明细日志（usage_refresh + failure_recovery）

Type: task
Status: resolved
Blocked by: 01

## 内容

在 ticket 01 的基础上，让两个定时任务每次执行的日志逐条点名供应商：

### usage_refresh（`src/lib.rs` handler + `src/usage/persist.rs::refresh_all_usage`）

- 刷新失败：`warn!(provider_id, provider_name, "供应商「{name}」用量刷新失败：{e}")`（replace 现 `用量刷新失败`）。
- 刷新成功：不逐条（用户拍板「变化+失败点名」）。
- 额度停用/恢复逐条行：已由 ticket 01 的 availability info 承载（含名 + label）。
- 汇总行：`用量刷新完成：成功刷新 {n} 家供应商用量`。
- `apply_usage_gate` 调用处把 `&p.name` 传入 availability 动作。

### failure_recovery（`src/lib.rs` handler + `src/proxy/failure_recovery.rs::recover_failure_disabled`）

候选供应商本来就少，逐条点名：

- 每个 `warn` 消息补 `供应商「{name}」` 前缀 + `provider_name` 字段（重新读取失败 / 无模型 / 无 Key / Key 解密失败 / 探测失败 / 状态更新失败 / 用量查询失败）。
- 新增「用量不可用被拦截」分支：`usage_allows_probe` 为 false 时 `warn!(provider_id, provider_name, "供应商「{name}」用量不可用，跳过自动恢复探测")`。
- 恢复成功逐条行：由 ticket 01 的 `recover_probe` info 承载（含名 + 子模型数）。
- 汇总行：`连续失败供应商恢复完成：成功恢复 {n} 家供应商`。

## 验收

- `src/usage/persist.rs::refresh_all_usage` 失败路径单测断言 message 含 `供应商「{name}」用量刷新失败`。
- `recover_failure_disabled` 各跳过/失败路径日志含供应商名（可在 `tests/provider_failure_recovery_integration.rs` 增断言或新增单测）。
- 现有测试签名与断言不回归。

## 参考

- spec.md 设计 C / D / E
- 测试：`src/usage/persist.rs::tests`、`tests/provider_failure_recovery_integration.rs`、`tests/provider_quota_gate_integration.rs`
