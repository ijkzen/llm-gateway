# 0007 — 调度计划单一所有者（scheduler::on_run_finished 统一回写）

## Status

accepted

## Context

定时任务的计划推进（`next_run_at`/`last_run_at` 回写）原先分散两处：worker 执行尾部自行计算并写库（含一组自研 next_run 断言），scheduler 在启停/更新时也重算——同一份「scheduled_at 锚定、超期从 now 重算」语义存在双实现，改动容易分叉。另外更新接口无条件重算 `next_run_at`：仅改标题/描述/启停也会在错过执行期把计划悄悄推后，与「错过的 cron 不补跑、@every 重启重计间隔」的既有语义相悖。

## Decision

1. 计划推进唯一实现收进 `cron/scheduler.rs` 的 `on_run_finished`（scheduled_at 锚定、超期从 now 重算、DB 回写与日志一并内聚）；worker 执行尾部不再自算/自写，只上报 `name`/`expression`/`scheduled_at`/`tz` 委托调用。
2. 更新接口（PUT）仅当表达式实际变更才重算 `next_run_at`；仅改标题/描述/启停不触碰计划。

## Consequences

- 单写者消除双实现分叉：原 worker 的 next_run 断言族经委托路径零迁移通过，后续计划逻辑改动只动 scheduler 一处。
- PUT 语义收敛：错过执行期后 title-only 更新不再悄悄推后计划，表达式变更仍正常重算到未来（集成回归锁定）。
- 执行回写与路由重算两条写入路径职责清晰：执行侧唯一写者 + 表达式变更侧条件重算。
