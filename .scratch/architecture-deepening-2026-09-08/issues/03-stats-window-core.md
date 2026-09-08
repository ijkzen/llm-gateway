# 03: 数据面板窗口核心——解析/边界收敛 + 纯函数（C3a）

**What to build:** stats.rs 先立「窗口核心」：三族窗口契约（summary/charts/insight 的可选参数家族、rank 的必填家族、四个 metrics 端点手抄校验家族）收敛为**一个** parse+validate（删 4 份同文案 400 错误与重复 WHERE 拼装）；分桶补零（fill_*_series 与 charts fill_trend 的 4 份副本）、percentile 插值、ratio 折算、bucket 起点算术（(window_start + tz*60_000)/bucket_ms 的 4 处重复）提为带单测的纯函数。`request_logs.rs` 的 end_time 边界语义与 stats 统一为半开区间（同表同前端日期选择器两种边界的问题消除；前端把「截止时刻」按半开下界语义标注）。端点行为（默认窗口回退、输出 JSON 形状）零变化，纯重构 + 测试迁移。

**Blocked by:** None（stats.rs 独立于 proxy 工单链，可并行）。

**Status:** ready-for-agent

- [x] 窗口解析家族收敛后行为等价（三族对同一输入给同一 start/end/粒度）
- [x] percentile / 补零 / ratio / bucket 起点改为纯函数并单测（当前零单测，仅 2490 行全栈集成兜底）
- [x] request_logs 边界改半开后，边界时刻（end_time 恰好命中）的过滤结果与 stats 一致，前端日期语义说明同步
- [x] 既有 stats 集成测试全绿（不改写场景语义）
- [x] 全量质量门绿

## Comments

- feat/stats-window-core 实施完成（质量门全绿：cargo test 790 / 33 套件，clippy 零警告，lint 219 文件，vitest 415）。提交留在分支未合 main。
- **落地形态**：① `ChartWindow::bucket_range()` 成为桶帧算术唯一实现，替换 charts/insight fill_int_series/fill_float_series/group_percentiles 四处逐字节相同副本（含 `.max(start+offset)` 守卫与 end-1 开区间语义，公式恒等搬移）；② `required_time_range` 收敛 parse_rank_query + 四个 metrics 端点的 5 份必填窗口校验（双语文案逐字节一致）；③ request_logs `end_time` 由闭区间改半开（`<=` → `<`）与 stats 全链路同语义——同表同日期选择器此前两种边界，现同一截止时刻过滤结果一致；前端无需改动（stats 本就是半开，日志页同一时刻语义随之对齐），前端时区口径统一留待 04。
- **双轴 code-review 结论**：Standards 轴 H1（required_time_range 插入导致 parse_rank_query 原 doc 块错挂）已修复归位；J3 桶帧测试近同义反复 → 改硬编码期望区间作独立预言（472230..=472233，首桶/末桶均手工复核）。Spec 轴通过并记录两处可接受部分收敛：① 补零 map 闭包（fill_*_series/fill_trend/percentile 填充）仍内联——抽出的正是重复本体（帧算术），闭包只剩一行 map+unwrap_or(0) 骨架；② 「三族收敛为一个 parse」措辞过誉——实际收敛的是必填族两分支（rank + 四 metrics），summary 的 both-or-neither 与 charts/insight 的可选回退语义各自不同、保持原样；③ 四个 metrics 端点各留 6 行 match 胶水（handler 错误类型各异，抽共用 helper 需改签名面，不值）。percentile 函数原文档（N·p）与实现（(n-1)·p）措辞不符为既有存量，未在本票扩大改动面。
- 集成验证：request_logs 时间段过滤用例、stats summary 半开边界用例全绿（38 + 8）；边界行恰好等于 endTime 时两端一致排除。