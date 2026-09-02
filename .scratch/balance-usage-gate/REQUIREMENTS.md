# 按量付费供应商余额耗尽自动禁用/恢复

## 需求描述

当前 `apply_usage_gate` 仅对订阅制（`billing_mode=1`）供应商生效：订阅额度耗尽时自动停用 Provider 及其全部虚拟模型子模型，恢复时自动启用。按量付费（`billing_mode=0`）供应商即使能查到余额、余额合计为 0，也不会被禁用，仅在 LB 选路时被临时剔除。

目标：将自动禁用/恢复机制扩展到能查到余额的按量付费供应商，余额为 0 时自动禁用，充值恢复后自动启用。

## 范围

### 受影响的供应商（余额可查的按量付费）

共 7 家（需确认所有已配置的相关实例均受影响）：

1. **Alibaba（阿里云百炼）** — dashscope.aliyuncs.com / dashscope-intl.aliyuncs.com，`cloud_balance::fetch_aliyun_bss`，AK/SK
2. **DeepSeek** — api.deepseek.com，`balance::fetch_deepseek`，API Key
3. **Moonshot（月之暗面）** — api.moonshot.ai / api.moonshot.cn，`balance::fetch_moonshot`，API Key
4. **OpenRouter** — openrouter.ai，`balance::fetch_openrouter`，API Key
5. **Volcengine（火山方舟按量）** — ark.cn-beijing.volces.com（/api/v3 路径），`cloud_balance::fetch_volcengine_billing`，AK/SK
6. **StepFun（阶跃按量）** — api.stepfun.ai / api.stepfun.com（普通路径），`balance::fetch_stepfun_account`，API Key
7. **Xiaomi（小米按量）** — api.xiaomimimo.com，`xiaomi::fetch_xiaomi_balance`，CookieCloud

### 判定口径

- `balance_usable()` 返回 `Some(false)`（查得到余额且合计 = 0）→ 自动禁用 Provider 及其全部虚拟模型子模型
- `balance_usable()` 返回 `Some(true)`（余额 > 0）且当前已禁用 → 自动恢复启用
- `balance_usable()` 返回 `None`（无法判定余额）→ 不下结论，保持原状

### 非目标

- 不改变 `balance_usable()` 的判定逻辑本身
- 不影响 LB 选路的剔除逻辑（当前已正确）
- 不影响订阅制供应商的现有门控逻辑
- 不涉及前端 UI 改动

## 实现思路

`src/usage/persist.rs::apply_usage_gate` 当前第一行 `if p.billing_mode != 1 { return Ok(()); }` 拦截了所有非订阅制供应商。

改为：
1. 对 `billing_mode == 1` 走现有 `subscription_usable()` 判定
2. 对 `billing_mode == 0` 走 `balance_usable()` 判定
3. 两种情况的停用/恢复动作一致（`set_provider_enabled` + `set_items_enabled`）

## 用户确认

用户已逐一确认全部 7 家可查余额的按量付费供应商余额归零后均自动禁用。