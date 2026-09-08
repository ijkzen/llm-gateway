# 0012 — 转发失败自愈管线：连续失败熔断、失败复查与自动恢复探测

## Status

accepted

## Context

单次上游故障（限流、5xx、连接失败）不该立刻把供应商踢出局，但连续失败说明供应商整体不可用，继续尝试只会拖慢请求并制造噪音；停用之后又不能永远停着——需要一条自动确认「已恢复健康」并回归选路的通道。可用性状态迁移本身收在 availability 模块（ADR-0003），本文记录触发与恢复的机制决策。

## Decision

1. **熔断**：内存、供应商粒度的连续失败计数（`availability::FailureCounter`）——该供应商任一转发请求失败加一（不论能否重试）、成功清零、进程重启清零；达到设置项 `max_consecutive_failures`（Int，默认 5，热生效）→ `availability::disable_for_failures`（原子条件更新：仅可用态 → `failure` + 级联停用 + warn 日志带 request_id，并发只触发一次）。转发侧只经 `on_forward_failure` 记数，不直接改状态。
2. **失败复查**（`proxy/failure_recheck.rs`）：成员失败后对开启用量查询的供应商异步发起实时用量核验，同一供应商 60 秒内节流（时间窗即去重，防失败风暴打爆用量接口）：耗尽 → 走 `apply_usage_gate` 停用（`quota` 态，恢复后 `usage_refresh` 自动恢复，ADR-0010）；充足 → 不动（留给计数路径）；抓取失败按无数据处理，不禁用也不影响计数；核验结果写回用量缓存供后续选路。
3. **自动恢复探测**（`proxy/failure_recovery.rs` + 内置任务 `failure_recovery`，`@hourly` 种子行）：枚举 `failure` 态供应商——可查用量者先经 `probe_gate` 确认仍有剩余（用量不可用跳过），再通过一次真实模型请求验证；成功 → `availability::recover_probe`（带 `expected_updated_at` 乐观锁，防探测期间状态已变时旧探测覆盖新状态）级联恢复并清零计数。

## Consequences

- 失败快速切走、健康自动回归，整条链路无需人工；`manual`/`quota` 态不被熔断/探测路径打扰（ADR-0003）。
- 计数与状态解耦：转发侧只记数触发，状态迁移收 availability 动作，规则（禁用不碰计数、恢复/手动启用清零）单处实现。
- 复查把「额度耗尽」从「故障」里分诊出来——前者可自动恢复（quota），后者走探测。

规格：`.scratch/lb-circuit-breaker/`、`.scratch/provider-failure-recovery/`。
