# 0008 — 用量预估抽为纯核心，信任边界显式化

## Status

accepted

## Context

`GET /api/providers/{id}/usage/estimate`（估算整个订阅周期 token 总量）的折算算术原先内联在路由 handler 里，与窗口选取、SQL 统计耦合，无法脱离 DB 直测。此前覆盖判定（按天 covered 桶）经历过两轮诊断回归（d807c27 → 645bca1，UTC 日桶对账与「完整过去日」缺口口径），「可预估」的信任边界需要与展示层诊断解耦并显式固化，防止算术静默漂移。

## Decision

1. 新建 `src/usage/estimate.rs`，纯函数核心（无 SQL/DB 依赖）：
   - `period_len_ms`：窗口长度（周 7 天 / 月 30 天）；
   - `quota_ratio`：已用比例——`used/limit` 优先、`used_percent` 兜底，非正/不可折算返回 None；
   - `is_estimatable`：信任边界显式化——网关记录已用 token > 0 且比例可折算才可预估（流量未全走网关时 0 记录不可折算，防直连静默低估）；按天覆盖检查不参与算术；
   - `estimated_total`：已用 token ÷ 已用比例，round。
2. handler 删除内联折算与 WEEK/MONTH 常量，只保留窗口选取（仅 weekly/monthly）、SQL 统计与响应装配。

## Consequences

- 折算算术 4 个直测单测直测，集成 5 场景保持全绿；口径改动只落 estimate.rs 一处。
- 信任边界成为单一谓词（`is_estimatable`），与「闲置日误杀/部分泄漏漏检」这类覆盖展示层诊断解耦（诊断语义见 CONTEXT「用量预估」词条）。
- 前端周窗 ×4 折月仍在展示层折算，核心只回答「该窗口周期总量」。
