# 01: Daily 订阅窗口贯通

**What to build:** 管理员可以在通用用量卡片看到真实的日额度，订阅负载均衡与额度门控也把 Daily 作为一等窗口处理，同时不破坏不含 Daily 的旧缓存和现有 5h/周/月供应商。

**Blocked by:** None (can start immediately).

**Status:** completed

- [x] 通用窗口类型和 API 输出支持 Daily，旧缓存 JSON 仍可读取。
- [x] 订阅 LB 比较顺序为 5h→日→周→月，同类多窗口仍取最差剩余值。
- [x] Daily 剩余为 0 时沿用现有不可用与额度门控规则。
- [x] 前端类型、中文/英文标签和用量卡片可展示 Daily 的剩余比例、金额与重置时间。
- [x] 后端排序/门控测试与前端组件测试先红后绿。

## Completion notes

红测确认 `WindowKind::Daily` 与界面标签缺失；实现后 `cargo test daily`、窗口槽位回归、ProviderUsageCard 11 个测试和 `tsc --noEmit` 通过。
