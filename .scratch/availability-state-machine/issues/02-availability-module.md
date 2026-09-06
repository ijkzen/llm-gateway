# 02: 新建 availability 模块：四个动作式入口与失败计数器迁入

**What to build:** 「供应商可用性」有了唯一 owner：模块提供 `disable_for_quota` / `disable_for_failures` / `enable_manual` / `recover_probe` 四个动作入口（探测的乐观锁语义收进 `recover_probe`），统一写入 `enable` + `disabled_reason` 镜像不变式（启用 ⇔ None）、级联停用/恢复虚拟模型条目（沿用 `cascade_disabled` 标记语义）、失败连击计数器（「禁用不碰、恢复与手动启用必须清零」成为模块内规则）。本票尚无调用方接入，线上行为零变化。

**Blocked by:** 01（模块读写 `disabled_reason`，依赖迁移已建列）。

**Status:** ready-for-agent

- [x] 四个动作入口对内存库表驱动验证禁用/启用组合矩阵（quota × manual × failure × 正常 的停用与恢复交叉）
- [x] 计数器规则被矩阵覆盖：停用不动计数器、恢复与手动启用清零
- [x] 恢复探测乐观锁：探测期间状态被更新则放弃恢复
- [x] 幂等：目标状态已达成时无副作用、无日志噪音
- [x] 全量质量门绿，现有行为无任何变化

## Comments

- 80e8e81（feat/availability-state-machine）实施完成；全量质量门绿（fmt/clippy -D warnings/cargo test 634 通过；前端无改动，主仓 lint+vitest 335 通过）。availability 模块落地：DisabledReason/FailureCounter（自 proxy 迁入）/七动作 + set_items_enabled 收拢；场景矩阵 11 测试。尚无调用方切换，行为零变化。
