---
Type: task
Status: ready-for-agent
Blocked by:
---

# 01-apply-balance-usage-gate

## Summary

修改 `apply_usage_gate`（`src/usage/persist.rs`）使其对按量付费（`billing_mode=0`）供应商也执行余额耗尽自动停用/恢复，与订阅制同构。

## Detail

当前 `apply_usage_gate` 第一行 `if p.billing_mode != 1 { return Ok(()); }` 对非订阅制供应商直接跳过。改为：

```rust
pub async fn apply_usage_gate(
    db: &DatabaseConnection,
    p: &provider::Model,
    data: &UsageData,
) -> Result<(), DbErr> {
    let usable = match p.billing_mode {
        1 => data.subscription_usable(),
        _ => data.balance_usable(),
    };
    let Some(usable) = usable else { return Ok(()) };
    // … 现有停用/恢复逻辑不变
}
```

## Tests

1. **单元测试**（`src/usage/persist.rs::tests`）：新增 `apply_usage_gate` 传 `billing_mode=0` + 余额耗尽（`balances=[0.0]`）→ 停用 Provider + 子模型；余额恢复（`balances=[50.0]`）→ 启用；空 balances（`balances=[]`）→ 不动。
2. **集成测试**（`tests/provider_quota_gate_integration.rs`）：新增 `seed_balance_provider` helper（`billing_mode=0`），全链路验证余额耗尽→禁用→恢复→启用。
3. **现有测试补充**：`gate_skips_unjudgeable_data` 中余额形态（`balances=[]` + `billing_mode=0`）不应翻转 enable 的断言。

## Seams

- 纯函数层：`apply_usage_gate` 单元测试（内存 DB，mock 数据）
- 全链路层：集成测试（内存 DB + 真实持久化调用）