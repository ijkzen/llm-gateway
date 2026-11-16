# 0028 — 模型级额度豁免：免费模型在额度耗尽后继续可用

## Status

accepted

## Context

订阅制供应商的额度耗尽后，部分**免费模型**仍可正常服务（厂商不把它们的调用计入订阅额度/余额）。ADR-0010 的额度门控是 Provider 级一刀切：任一已提供窗口剩余为 0 → `disable_for_quota` 把 Provider 置为 `quota` 停用并级联停用名下全部虚拟模型条目。于是免费模型也被一起切断，虚拟模型里明明还有可用成员却整体降至其他供应商。

「这个模型是否消耗订阅额度」的属性属于 `provider_model`（同一 Provider 下不同模型口径不同），不属于 Provider，也不适合塞进加密的 `provider.extra` 文本。判定也不能靠价格自动推断：models.dev 目录的 `cost=0` 与「不扣订阅额度」不是同一件事（例如 Copilot 的免费模型仍可能出现在价格目录里），且本仓库 vendor 的目录快照未保留 cost 字段。

## Decision

1. **模型级手动标记**：迁移 30 给 `provider_model` 增加 `quota_exempt boolean NOT NULL DEFAULT 0`。管理后台供应商模型详情可开关（文案「不计入额度」），模型列表卡片显示「免额度」徽标；备份导出/导入携带该字段（旧备份缺省 false）。不做基于目录价格/模型名的自动推断——人工标记是唯一事实源，避免误判把计费模型当免费模型打满额度。
2. **门控按模型粒度降级**：`apply_usage_gate` 判定不可用且该 Provider 存在任一 `quota_exempt=true` 模型时，不再调用 `disable_for_quota`，改走 `availability::gate_items_for_quota_exempt`：
   - Provider 保持启用（若原处于 `quota` 停用态则原子解除，仅解除 quota）；
   - 只停用「非豁免模型」对应的 `virtual_model_item`，并打 `cascade_disabled` 标记；
   - 豁免模型的条目保持/恢复启用（只恢复带级联标记的，用户手动关闭的不动）；
   - 额度恢复且离开边界区后，`recover_quota_exempt_items` 按标记恢复被停用的条目。
   没有任何豁免模型时，行为与 ADR-0010 完全一致（整家停用/恢复）。
3. **选路剔除保留豁免成员**：LB 策略 0/1 在 `subscription_usable()==false` 时只剔除**非豁免**订阅成员，`quota_exempt` 成员继续参与选路（`subscription_member_usable`）。条目级停用是缓存放久的兜底：即使用量缓存过期/抓取失败导致 LB 无法判定，已停用的计费条目也不会被重新选中。
4. **边界探活避开免费模型**：`probe_preamble` 优先选非豁免且 `model_id` 最小的模型；若全 Provider 均为豁免模型则跳过订阅额度探活。否则免费模型探活成功会把已耗尽的计费额度误判为恢复，重新打开计费模型流量。
5. **可观测性**：部分门控在发生真实变更时写定时任务日志（停用条目数 + 保留免费模型数）；不新增飞书转换通知——Provider 的启用状态没有变化，ADR-0023 的「自动停用/启用」口径不适用。

## Consequences

- 免费模型在订阅额度耗尽后继续服务，虚拟模型不会因单个 Provider 额度见底而整体失去该 Provider 的全部成员。
- 计费模型的保护是双层的：选路层按 `subscription_usable` 剔除（缓存新鲜时），条目层持久停用（缓存过期/抓取失败时仍有效）。
- 连续失败熔断仍是 Provider 级：免费模型被上游限流/报错仍可能把整家 Provider 打成 `failure` 停用。若实际出现，需要单列模型级失败计数的议题，不在本决策范围内。
- 模型详情开关是人工操作：新导入的免费模型默认计费口径，管理员需要显式标记。
- 测试：`tests/provider_boundary_probe_integration.rs` 覆盖整家停用、部分停用、quota→部分过渡、探活失败保留免费模型与全豁免跳过探活；LB 谓词由 `proxy::lb::tests` 锁定。
