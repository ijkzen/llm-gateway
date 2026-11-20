# 0029 — 豁免标记即时生效：保存模型后立即重跑门控与成员状态透传

## Status

accepted

## Context

ADR-0028 实现后仍有两个缺口（2026-11-20 生产实证）：

1. **生效延迟**：管理后台给已被整家 `quota` 停用的 Provider 勾选「不计入额度」后，Provider 不会当场翻正——保存接口只写 `provider_model.quota_exempt`，门控要等下一轮 `usage_refresh`（5 分钟定时器）才重跑。期间 `load_members` 在 `quota_exempt` 判断之前用 `traffic_available(p)` 硬性过滤 `provider.enable=false` 的成员，免费模型实际收不到 `/v1` 流量，请求返回「没有可用的成员」。进程重启会重置计时器，进一步放大等待时间。
2. **成员状态不可判定**：虚拟模型成员响应（`VirtualModelItemResponse`）不携带 `quota_exempt` 或供应商停用原因，前端只能按 `providerEnable` 一刀切打「随供应商禁用」标签。对免费模型而言，这个标签在额度耗尽场景下误导（用户以为免费模型也不可用），却无法区分 manual/failure 停用（那种情况下确实不可用）。

## Decision

1. **标记变化即重跑门控**：provider_model 的四个写接口（create / batch create / update / delete）在 `quota_exempt` 发生变化或新建/删除免费模型时，调用 `usage::persist::regate_after_model_change`：加载 Provider → 校验 `usage_enabled` → 读新鲜用量缓存（`read_usage_cache`，不发厂商网络请求）→ 重跑门控。无缓存/未开启用量查询时跳过，由下一轮定时刷新兜底；门控失败只记 warn，不让「模型已保存」返回 5xx。取消标记同样重跑，以便额度仍耗尽时重新整家停用。若 Provider 当前已是 `quota` 停用态且存在免费模型，直接调用 `gate_items_for_quota_exempt` 切到模型级门控——因为边界区（剩余 0~1%）的常规门控把裁决权留给探活，不会翻正 Provider，免费模型仍会被 `traffic_available` 挡住。
2. **成员响应透传豁免状态**：`VirtualModelItemResponse` 增加 `quota_exempt`（来自 `provider_model`）与 `provider_disabled_reason`（来自 `provider.disabled_reason`），camelCase 为 `quotaExempt` / `providerDisabledReason`。四处返回该响应的 handler 共用 `load_item_responses`，一处填充全覆盖。
3. **前端区分展示**：虚拟模型成员卡片与详情弹窗对 `quotaExempt=true` 的成员展示「免额度」徽标；仅当 `providerDisabledReason === "quota"` 时不显示「随供应商禁用」（因为按 ADR-0028 免费模型本应可用，门控会即时翻正）。manual/failure 停用仍照实标注——那种状态下免费模型确实无法参与选路，不能掩盖。模型编辑弹窗同样补上「免额度」徽标。

不改变 ADR-0028 的语义：判定口径、`gate_items_for_quota_exempt` / `recover_quota_exempt_items` 的行为、LB 选路谓词均不动；本决策只消除「标记后要等下一轮」的时间窗口和展示层的信息缺失。

## Consequences

- 勾选「不计入额度」保存后立即恢复服务，无需等待或重启；取消标记后立即重新整家停用（若额度仍耗尽）。
- 用量缓存过期/缺失时仍无法即时判定，退化为原有时延（定时刷新兜底）；不为即时性在管理请求路径引入厂商网络调用。
- 前端不再把「额度停用中的免费模型」误标为「随供应商禁用」，但保留了 manual/failure 停用下的真实告警。
- 成员响应新增两个字段为纯 additive，不破坏既有消费者。
- 测试：`tests/provider_boundary_probe_integration.rs` 覆盖即时翻正与无缓存空操作；`tests/virtual_models_integration/members.rs` 覆盖字段透传；前端覆盖免费模型在 quota/manual 两种停用原因下的标签差异。
