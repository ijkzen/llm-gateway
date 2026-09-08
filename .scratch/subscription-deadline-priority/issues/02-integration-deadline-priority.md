# 02: 集成链路「截止日期优先」测试

**What to build:** 在真实 LB 链路上验证截止日期优先生效：策略 0 订阅组成员 A 5h 剩余高（80%）但周截止远、B 5h 剩余低（20%）但周截止近 → 转发请求应命中 B（截止日期优先于剩余百分比），断言 mock 上游收到请求的是 B 而不是 A。

**Blocked by:** 01（订阅制比较器重写 + 单测）

**Status:** ready-for-agent

- [ ] proxy_integration.rs 新增测试（沿用 mock 上游 + 预置用量缓存模式）
- [ ] 断言：截止近者（5h 剩余低）被优先转发
- [ ] 现有 `subscription_first_ranks_by_remaining_five_hour_usage`（无截止时间 → 兜底路径）保持绿
