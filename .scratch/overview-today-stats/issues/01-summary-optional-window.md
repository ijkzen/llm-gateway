# 01: summary 端点支持可选时间窗口参数

**What to build:** `/api/stats/summary` 保持无参数返回全历史累计（向后兼容）；新增可选
`startTime`/`endTime`（毫秒时间戳）查询参数，两者同时提供且 `endTime > startTime` 时，只按
`request.start_time` 半开区间 `[start, end)` 聚合——4 个指标（请求数/成功率/总 Token/缓存命中率）
与聚合口径不变。任一参数缺失或 `endTime <= startTime` 返回 400。HTTP 集成测试覆盖三态
（无参数全量、带区间、非法参数）。

**Blocked by:** None（可立即开始）

**Status:** ready-for-agent

- [ ] 无参数响应与改动前完全一致（既有 summary 集成测试保持通过）
- [ ] 带 startTime/endTime 只聚合区间内请求，比率口径与全量一致（跨区间种子数据）
- [ ] 只传一端 / endTime <= startTime → 400
- [ ] 后端 fmt + clippy + 该测试文件绿
