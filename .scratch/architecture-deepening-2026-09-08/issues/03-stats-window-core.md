# 03: 数据面板窗口核心——解析/边界收敛 + 纯函数（C3a）

**What to build:** stats.rs 先立「窗口核心」：三族窗口契约（summary/charts/insight 的可选参数家族、rank 的必填家族、四个 metrics 端点手抄校验家族）收敛为**一个** parse+validate（删 4 份同文案 400 错误与重复 WHERE 拼装）；分桶补零（fill_*_series 与 charts fill_trend 的 4 份副本）、percentile 插值、ratio 折算、bucket 起点算术（(window_start + tz*60_000)/bucket_ms 的 4 处重复）提为带单测的纯函数。`request_logs.rs` 的 end_time 边界语义与 stats 统一为半开区间（同表同前端日期选择器两种边界的问题消除；前端把「截止时刻」按半开下界语义标注）。端点行为（默认窗口回退、输出 JSON 形状）零变化，纯重构 + 测试迁移。

**Blocked by:** None（stats.rs 独立于 proxy 工单链，可并行）。

**Status:** ready-for-agent

- [ ] 窗口解析家族收敛后行为等价（三族对同一输入给同一 start/end/粒度）
- [ ] percentile / 补零 / ratio / bucket 起点改为纯函数并单测（当前零单测，仅 2490 行全栈集成兜底）
- [ ] request_logs 边界改半开后，边界时刻（end_time 恰好命中）的过滤结果与 stats 一致，前端日期语义说明同步
- [ ] 既有 stats 集成测试全绿（不改写场景语义）
- [ ] 全量质量门绿

## Comments
