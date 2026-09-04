# Spec: 数据面板首页「今日」指标

## Problem Statement

数据面板首页顶部目前只有 4 张**全历史累计**指标卡（累计请求数 / 成功率 / Token 总数 / 缓存命中率），
用户想立刻看到「今天到现在」的同口径数据，而不必在图表区手动切窗口推算。每次要看今日表现，目前没有直接入口。

## Solution

在首页顶部累计卡正下方增加一排 **4 张「今日」指标卡**，指标口径与累计卡完全一致，只是统计窗口
从「全历史」变为「浏览器本地自然日（今天 0 点 → 当前时刻）」。
后端复用现有 `GET /api/stats/summary` 聚合逻辑，新增**可选**的 `startTime`/`endTime` 时间过滤参数；
不带参数时行为与今天完全一致（向后兼容）。

## User Stories

1. 作为网关运维者，我想在数据面板首页一眼看到**今天**的累计请求数，以便快速判断今日调用量趋势。
2. 作为网关运维者，我想在数据面板首页一眼看到**今天**的请求成功率，以便判断今日服务是否健康。
3. 作为网关运维者，我想在数据面板首页一眼看到**今天**消耗的 Token 总量，以便跟踪今日用量。
4. 作为网关运维者，我想在数据面板首页一眼看到**今天**的缓存命中率，以便评估今日缓存效果。
5. 作为网关运维者，我希望「今日」指标与上方「累计」指标含义、格式完全一致，只是统计周期不同，
   这样不需要重新学习卡片含义。
6. 作为网关运维者，我希望「今日」的时区口径与首页已有「今天」图表窗口一致（按我的本地时区），
   避免同一天在不同时区理解下数值不同。
7. 作为后端 API 使用者，我希望 `/api/stats/summary` 在**不传时间参数时行为不变**（返回全历史累计），
   这样既有调用不会被破坏。
8. 作为后端 API 使用者，我希望 `/api/stats/summary` 在传入 `startTime`/`endTime` 时返回该区间的
   同口径聚合，这样我可以复用它查询任意时间段（不限于今日）。
9. 作为前端开发者，我希望「今日」卡与累计卡复用同一数据源与类型，避免为今日另维护一套口径。

## Implementation Decisions

### 后端：`GET /api/stats/summary` 加可选时间参数

- 请求参数（均为可选，`camelCase` query）：`startTime`（毫秒时间戳，含）、`endTime`（毫秒时间戳，不含）。
- 二者**要么都缺省、要么都提供**：
  - 都缺省 → 与现状完全一致：全表聚合，SQL 不变。
  - 都提供且 `endTime > startTime` → 追加 `WHERE start_time >= ? AND start_time < ?`（半开区间，
    复用 `request.start_time` UTC 毫秒时间戳）。
  - 只提供其一，或 `endTime <= startTime` → 返回 400（参数非法），保持与仓库其他 query 参数校验风格一致。
- 聚合字段与口径**完全不变**：请求数 `COUNT(*)`、成功率 `SUM(success)/COUNT(*)`、总 Token
  `SUM(total_tokens)`、缓存命中率（加权）`SUM(input_cache_tokens)/SUM(input_tokens)`，NULL 处理与现状一致。
- 成功/缓存比率继续走现有 `weighted_ratio`（`round_5` 5 位小数），不引入新口径。
- 返回结构 `SummaryResponse`（totalRequests/successRate/totalTokens/cacheHitRate）不变。

### 前端

- 数据 hook：`useDashboardSummary` 增加可选时间参数；其 query key 需把参数纳入，使「累计」与「今日」
  成为两个独立缓存条目，互不 invalidate。
- 页面：顶部改为**两行卡片**，每行 4 张、沿用现有 `StatsCard` 网格：
  - 第一行：累计（现状 4 卡，副标题「全部历史」不变）。
  - 第二行：今日（同一 4 个指标，副标题「今日」/「Today」）。
- 今日窗口边界 = 现有「day 周期」语义：`periodBounds("day", 0, now)` → `[本地今日 0 点, now]`
  （当前周期终点截到 `now`）。**复用** `race-period.ts` 的 `periodBounds`，不新写边界函数，保证与图表区口径一字不差。
- 今日卡数值/图标/标签与累计卡一一对应，仅副标题区分。

### i18n

- 新增文案键（en / zh-CN 两文件同步）：今日卡副标题「今日」/「Today」。

## Testing Decisions

- **缝 1（后端）**：`tests/stats_integration.rs` 的 summary 相关 describe——用真实 SQLite + HTTP 走
  `/api/stats/summary`，断言：
  - 不传参数 = 全历史（既有用例已覆盖，保持通过即向后兼容证明）。
  - 传入区间：只聚合区间内请求（跨区间种子数据），比率口径与现状一致。
  - 非法参数（只传一端 / `endTime <= startTime`）→ 400。
- **缝 2（前端）**：`web/src/__tests__/overview-page.test.tsx`——页面级 mock 数据 hooks，断言：
  - 首屏渲染 8 张卡：第一行累计（副标题全部历史）、第二行今日（副标题今日），值格式一致。
  - 今日 summary 请求携带的 `startTime`/`endTime` 等于本地今日窗口（用现有测试对 window 的断言方式）。
- **不做**：后端无 SQL 层单测（聚合逻辑只有一条 SQL，缝在集成测试最经济）；
  前端无组件级单测（页面测试已覆盖渲染与传参）。
- 既有代码风格：参考现有 summary 集成测试与 overview-page 页面测试的断言/辅助函数写法。

## Out of Scope

- 后端独立的 `/summary/today`（或任何固定「今日」语义端点）——时区边界由前端算并传参。
- 支持「昨日 / 本周 / 本月」等其他时间段 summary 参数组合的业务 UI（后端参数本身可被任意区间调用，
  属 API 自然能力，不在本需求额外做 UI）。
- 今日/累计同卡双值合并布局。
- 任何 `request` 表 schema 变更、索引变更、迁移。
- 改动 `charts`/rank/metrics/insight 等其他 stats 端点。
- 首页现有图表的时区/窗口逻辑改动。

## Further Notes

- 全站统计的时区口径统一为**浏览器本地时区**：图表按客户端 `tzOffsetMinutes` 分桶，今日边界由前端
  本地日期 API 计算。后端 `summary` 的时间过滤按 UTC 毫秒时间戳，不做时区换算——时区语义全部留在客户端，
  与 `charts` 端点一致。
- 当前周期「天」窗口终点截到 `now`（而非明天 0 点）是 `race-period.ts` 既有语义，直接继承，
  使今日卡与「今天」图表窗口数值可对得上。
