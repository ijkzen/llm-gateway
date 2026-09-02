---
Status: ready-for-agent
Slug: balance-usage-gate
---

# 按量付费供应商余额耗尽自动禁用/恢复

## Problem Statement

按量付费（`billing_mode=0`）供应商即使开启了用量查询、能查到余额，余额合计归零后 Provider 也不会被自动禁用，只是请求时被 LB 选路临时剔除。用户希望这类供应商在余额耗尽后能像订阅制一样被自动停用（连带停用其全部虚拟模型子模型），避免继续把请求路由到一个已经没钱的账户上；充值恢复后自动重新启用。

## Solution

将 `apply_usage_gate` 的自动停用/恢复机制从「仅订阅制」扩展到「能判定余额的按量付费供应商」：余额合计为 0 时自动停用 Provider 及其全部虚拟模型子模型，余额恢复后自动启用。判定口径复用现有 `UsageData::balance_usable()`。

## User Stories

1. 作为管理员，我希望 Alibaba（百炼）按量账户余额归零后该 Provider 被自动停用，以便请求不再路由到已无余额的账户。
2. 作为管理员，我希望 DeepSeek 按量账户余额归零后该 Provider 被自动停用，以便避免无谓的上游调用。
3. 作为管理员，我希望 Moonshot 按量账户余额归零后该 Provider 被自动停用。
4. 作为管理员，我希望 OpenRouter 按量账户余额归零后该 Provider 被自动停用。
5. 作为管理员，我希望 Volcengine 火山方舟按量账户余额归零后该 Provider 被自动停用。
6. 作为管理员，我希望 StepFun 阶跃按量账户余额归零后该 Provider 被自动停用。
7. 作为管理员，我希望 Xiaomi 小米按量账户余额归零后该 Provider 被自动停用。
8. 作为管理员，我希望上述供应商在充值、余额恢复后被自动重新启用，以便无需手动操作即可恢复服务。
9. 作为管理员，我希望对查询不到余额（无法判定）的按量供应商不做任何动作，以便避免上游抖动误伤。
10. 作为管理员，我希望停用动作与订阅制一致：连带停用该 Provider 名下的全部虚拟模型子模型，以便请求不再选中它。

## Implementation Decisions

- 修改 `apply_usage_gate`（`src/usage/persist.rs`）的入口分流：不再是 `billing_mode != 1` 直接返回，而是按计费模式/数据形态选择判定谓词：
  - `billing_mode=1`（订阅制）→ `UsageData::subscription_usable()`
  - `billing_mode=0`（按量付费）→ `UsageData::balance_usable()`
  - 无法判定（`None`）→ 不做任何动作
- 停用/恢复动作不变：复用 `provider_repo::set_provider_enabled` + `set_items_enabled`，日志文案区分订阅/余额。
- **不硬编码 7 家供应商名单**：按数据形态分发后，凡能产出 `UsageKind::Balance` 数据的按量供应商（即当前 7 家可查余额的供应商）自动纳入；新增的余额型供应商也能自动生效。
- 不改 `balance_usable()` 判定逻辑、不改 LB 选路剔除逻辑、不改前端。

## Testing Decisions

- 好的测试：只验证外部行为——给 `apply_usage_gate` 传入某种计费模式 + 某种用量数据，断言 Provider 与虚拟模型子模型的 `enable` 是否翻转；不测内部实现细节。
- **单元测试**（`src/usage/persist.rs::tests`）：新增 `apply_usage_gate` 传 `billing_mode=0` + 余额耗尽/恢复数据的用例，验证停用与恢复；余额无法判定（空 balances）时不动。
- **集成测试**（`tests/provider_quota_gate_integration.rs`）：复用 `seed_subscription_provider`（把 `billing_mode` 改为 0）新增余额耗尽→禁用、恢复→启用的全链路用例。
- **现有测试补充**：`gate_skips_unjudgeable_data` 增加余额形态（空 balances）+ `billing_mode=0` 供应商不应翻转 enable 的断言。
- 先例：现有 `quota_exhaustion_disables_and_restore_reenables` 已覆盖订阅制的禁用/恢复闭环，本改动与其同构。

## Out of Scope

- 不改 `balance_usable()` / `subscription_usable()` 的判定逻辑。
- 不改 LB 选路的剔除行为（当前已正确）。
- 不改订阅制门控的既有行为。
- 无前端、无 schema、无 API 变更。

## Further Notes

- 用户已逐一确认全部 7 家可查余额的按量付费供应商余额归零后均自动禁用（AskUserQuestion，2026-09-02）。
- 生产环境本地 dev 库当前只配置了订阅制供应商（OpenCode Zen），本次改动对存量数据无影响，仅影响后续开启用量查询的按量供应商。
