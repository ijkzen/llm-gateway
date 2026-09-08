# 03: Command Code 月窗截止时间补取

**What to build:** Command Code 用量获取补上被遗漏的月窗截止时间：subscriptions 接口实际返回 `currentPeriodEnd`（订阅周期结束 = 月度 credits 重置点），现有代码只取 `planId` 与 `currentPeriodStart`（后者仅用于 usage/summary 的 since），月窗 resets_at 硬编码为 None。改为解析 `currentPeriodEnd` 并传给月窗窗口构造，使月窗 resets_at 可用（参与截止链排序、前端展示重置时间）。

**Blocked by:** None（可立即开始）

**Status:** ready-for-agent

- [ ] subscriptions 解析增加 `currentPeriodEnd` 并传入 `parse_command_code_credits`
- [ ] 月窗 `QuotaWindow` resets_at 由硬编码 None 改为周期结束时间
- [ ] 单测：fixture 含 `currentPeriodEnd` → 月窗 resets_at 有值；缺省时保持 None（回归）
- [ ] `cargo test` 该模块全绿
