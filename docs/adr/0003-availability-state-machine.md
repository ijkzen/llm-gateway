# 0003 — 供应商可用性状态机与 disabled_reason 列

## Status

accepted

## Context

供应商「能不能用」由 `enable` + `failure_disabled` 两列表达，但改变状态的规则散落在四处：转发失败路径（`note_member_failure` 连击判断与停用）、额度门控（`apply_usage_gate` 自动停用/恢复）、管理员手动启停（`routes/providers.rs` 更新 handler 内联清 `failure_disabled` 与级联）、恢复探测（`recover_provider_from_failures`）。内存失败连击计数器的复位也分散在三处，漏写即 bug。

两列无法区分「额度耗尽停用」与「手动停用」（均为 `enable=false, failure_disabled=false`），导致现存缺陷：手动停用的供应商（开了用量查询）会被 `usage_refresh` 在额度判定充足时自动重新启用，覆盖管理员决定。

## Decision

1. 迁移 21 新增 `disabled_reason` 列并完成存量回填，迁移 22 再删除被取代的 `failure_disabled` 列（先加后删：回填语句依赖旧列仍可读）。四值：`None`（正常）/ `failure`（连续失败禁用）/ `quota`（额度耗尽）/ `manual`（手动停用）。`enable` 列保留作镜像，不变式「启用 ⇔ reason=None」由新模块统一写入保证。
2. 存量回填：`failure_disabled=1` → `failure`；`enable=false` 且非失败禁用 → `manual`（宁多一次手动启用，不抢管理员决定权）；启用行 → `None`。
3. 新建 `src/availability.rs`，动作式入口：`disable_for_quota` / `recover_quota`（额度门控：usage_refresh 与失败复查共用）、`disable_for_failures` / `recover_probe`（熔断与自动恢复探测，保留 `expected_updated_at` 乐观锁）、`disable_manual` / `enable_manual`（管理员手动启停）、`on_forward_failure`（转发失败入口：连击计数 + 达阈值熔断）；读侧「选路可用」统一经谓词 `traffic_available`（启用 ∧ 无停用原因，见 CONTEXT.md「选路可用」）。级联停用/恢复虚拟模型条目沿用 `set_items_enabled` 的 `cascade_disabled` 标记语义。失败连击计数器类型（`FailureCounter`）迁入该模块（AppState 的 `failure_counter` 字段类型随之更换），「禁用不碰计数器、恢复与手动启用必须清零」成为模块内规则。
4. 额度刷新只对 `quota` 态做自动恢复，对 `manual` 态完全不动（修复上述缺陷）；手动启用一个额度仍耗尽的供应商不设豁免，下轮刷新会再次停用并标记 `quota`（与现状一致）。

被否决的备选：保持两列三态（无法表达修复，缺陷保留）；复用 `failure_disabled` 标记手动停用（与恢复探测语义冲突）；删除 `enable` 以 reason 作单一事实源（LB 选路、多处查询过滤与前端契约都要跟着改，改动面不成比例）。

## Consequences

- 「手动停用被额度刷新覆盖」缺陷随迁移修复；升级后历史禁用行不会被自动启用。
- 所有可用性迁移规则集中在 `availability.rs`，新增状态/动作只改一处；3×3 禁用/启用组合可表驱动单测，不再依赖整链路集成测试。
- 前端与 `/v1` 选路零改动（`enable` 语义不变，`failure_disabled` 本就未暴露给前端）。
- 部署后需 `PRAGMA table_info(provider)` 验证列变更生效（schema_migrations 版本守卫）。
- 术语见 CONTEXT.md「停用原因 (Disabled Reason)」。
