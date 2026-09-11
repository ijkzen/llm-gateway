# 0015 — 统计口径单一来源：窗口桶帧、半开边界与设置表时区

## Status

accepted

## Context

数据面板有多个聚合端点（summary/charts/insight/metrics/request_logs），窗口分桶与必填窗口校验曾在多处逐字节复制（小时/天桶帧算术 4 份相同副本），边界语义还不一致：`request_logs` 用闭区间、stats 用半开，同一截止时刻两边过滤结果不同。时区口径早期由客户端 `tzOffsetMinutes` 提供，管理后台视角随浏览器时区漂移，与 cron 的服务器时区语义（东八区）也脱节。

## Decision

1. 窗口桶帧唯一实现 `ChartWindow::bucket_range`（charts/insight 的 fill 系列与分位聚合全部委托，行为恒等重构）；必填窗口校验收敛为单一 `required_time_range`（5 份手抄文案删除）。
2. 同表同边界：`request_logs` 的 end_time 由闭区间改半开（`<`）——与 stats 一致，对同一截止时刻给出相同过滤结果（行为变更仅此一处且为有意对齐）。
3. 时区单一来源 = 设置表 `timezone`（IANA 校验，缺省 `Asia/Shanghai`；`language`/`timezone` 为受保护键）：分桶偏移与周期窗口（今日午夜、天/月/年边界）同口径；纯函数 `tz_offset_minutes_at` 按窗口起点时刻定偏移（DST 时区用起点偏移分桶，默认东八区无 DST）；客户端 `tzOffsetMinutes` 参数删除、不再参与语义。无时区厂商的用量重置时间字符串同源解释（`timezone_sync` 进程内副本随设置热更新）；timezone 变更时重载 cron 任务。
4. `GET /api/stats/summary` 支持可选 `startTime`/`endTime`（半开），缺省保持全量历史聚合（向后兼容）；新增聚合端点（`/api/stats/insight` 等）一律复用 request 表现有字段，零 schema 变更。

## Consequences

- charts/insight/排行/请求日志同窗同桶同时区，前端管理后台为单一时区视角（前端窗口推导见 ADR-0004 收敛后的 race-period/stats hooks）。
- 聚合口径改动只落 stats 层一处（窗口核心 + 时区函数），并有纯函数直测。
- 接口 JSON 形状与缺省行为稳定，客户端只需跟随设置表时区。
- 前端把窗口解析结果放进 query key 导致的「重取复用旧窗口」问题，见 ADR-0022（窗口身份与取值分离）。

规格：`.scratch/overview-today-stats/`、`.scratch/architecture-deepening-2026-09-08/`（03 窗口核心 / 04 时区统一）。
