# 0010 — 额度门控：订阅额度/按量余额耗尽的自动停用与恢复

## Status

accepted

## Context

订阅额度耗尽或按量余额耗尽后继续把流量发往该供应商，只会浪费 failover 尝试并积累无谓失败。门控需要与 LB 剔除、选路可用判定共用同一份「额度是否还有」的口径（否则一个说可用一个说耗尽，行为分叉）；历史上还出过「手动停用被额度刷新自动恢复覆盖」的缺陷（见 ADR-0003）。

## Decision

1. `usage_refresh` 定时刷新与转发失败复查（ADR-0012）共用 `persist::apply_usage_gate`：订阅制任一已提供窗口剩余为 0，或按量余额查得到且合计为 0 → `availability::disable_for_quota`（`enable=false` + `disabled_reason=quota` + 级联停用虚拟模型条目并打 `cascade_disabled` 标记）；查不到余额（无法判定）→ 保持原状。日志文案区分「订阅额度」与「余额」来源。
2. 自动恢复：不可用态消失（订阅全部窗口剩余 > 0 / 余额 > 0）→ `availability::recover_quota`（仅解除 `quota` 态，幂等条件更新 + 级联恢复带标记条目）。`manual`/`failure` 态不被额度刷新触碰（ADR-0003 决定 4），手动停用不设豁免——额度仍耗尽时下轮刷新会再次停用并标记 `quota`。
3. 监测范围：`usage_refresh` 不过滤 enable——停用中的供应商持续刷新，额度一恢复即自动反向启用，无需人工介入。

## Consequences

- 额度耗尽即停、恢复即启，全自动闭环；判定口径与选路剔除同源（`subscription_usable`/`balance_usable`，见 ADR-0011），无「剔除说耗尽、门控说可用」的分叉。
- 状态迁移统一走 availability 动作（幂等/级联/计数规则收口于 ADR-0003 模块），门控只管「判定 + 触发」。
- 停用/恢复事件在定时任务日志里点名供应商，可观测。

规格与测试：`.scratch/balance-usage-gate/`、`tests/provider_quota_gate_integration.rs`。
