# REQUIREMENTS — 定时任务明细日志（供应商粒度）

## 需求原文（用户，2026-09-07）

> 为定时任务添加更多明细，现则两个定时任务都要添加，哪些哪些供应商发生了什么状态变更都要写出来，不要单单只写个数字。

## 现状（已核对代码）

两个内置定时任务（`src/cron/seed.rs`）的 handler 只输出聚合计数，看不出「哪家供应商发生了什么」：

1. **`usage_refresh`**（`@every 5m`，handler 在 `src/lib.rs:161`）
   - 收尾只打一条 `用量刷新完成，成功刷新 {n} 家供应商` —— 纯数字。
   - `refresh_all_usage`（`src/usage/persist.rs`）内部：成功路径不记日志；失败路径 `warn!(provider_id, "用量刷新失败")` 有 id 无名。
   - 状态变更由 `apply_usage_gate` → `availability::disable_for_quota` / `recover_quota` 打 `info!(provider_id, items, ...)`，消息里没有供应商名（且结构化字段不会渲染进任务日志 UI，见下）。
2. **`failure_recovery`**（`@hourly`，handler 在 `src/lib.rs:190`）
   - 收尾只打一条 `连续失败供应商恢复完成，成功恢复 {n} 家供应商` —— 纯数字。
   - `recover_failure_disabled`（`src/proxy/failure_recovery.rs`）内跳过/探测失败日志有 `provider_id` 无名；恢复成功由 `availability::recover_probe` 打 `info!(provider_id, items, ...)`，同样无名。

### 关键约束：任务日志 UI 只渲染 message 文本

`src/cron/log_capture.rs` 的 `MessageRecorder` 只提取事件 `message` 字段；`provider_id`/`items` 等结构化字段**不会**出现在任务日志界面。因此要让用户看到「哪家供应商」，供应商名必须拼进 message 文本（结构化字段仅进 JSON 文件日志，不满足需求）。

## 决策（已拍板）

- **D1 明细落在任务执行日志**：在两个任务的执行日志里逐条列出供应商粒度结果与状态变更；不新增表、不动前端。
- **D2 供应商标识进 message**：消息统一带 `供应商「{name}」`，name 为主（用户视角）；结构化字段另带 `provider_id`/`provider_name` 供 JSON 文件日志。
- **D3 状态变更日志仍归 availability 状态机**（ADR-0003「各动作自带幂等、级联与结构化日志」）：给 `disable_for_quota` / `recover_quota` / `recover_probe` 增加 `provider_name: &str` 参数，把名字拼进各自 info 消息。改动面最小（三个函数实际调用点分别只有 `apply_usage_gate` 与 `recover_failure_disabled`）。
- **D4 usage_refresh 粒度 =「变化 + 失败点名」**（用户拍板，AskUserQuestion 2026-09-07）：
  - 停用/恢复/刷新失败：逐条点名（供应商名 + 原因/错误）。
  - 刷新成功：不逐条，保留收尾聚合行 `成功刷新 {n} 家`。
- **D5 failure_recovery 逐条点名**：每小时一次、候选少，每家候选供应商的探测结果（恢复成功 / 跳过及原因 / 探测失败 / 被用量门控拦截）均含名。
- **D6 不扩大范围**：`disable_for_failures`（转发链路熔断，非定时任务，调用点无现成 name）与手动启停日志不改；`usage_refresh` / `failure_recovery` 之外的任务不动。

## 非目标（ponytail 裁剪）

- 不加数据库列/迁移（无状态要持久化，日志即交付物）。
- 不改前端（任务日志弹窗已实时展示 message）。
- 不引入日志聚合/统计结构体（保持 `refresh_all_usage` / `recover_failure_disabled` 返回 `usize` 签名不变）。
- 不替 `disable_for_failures` 补 name（不在两个定时任务路径，且无成本收益）。

## 验收口径

- `usage_refresh` 单次执行日志：每家目标供应商至少一条成功/失败行（含名）；发生停用/恢复时另有状态变更行（含名与原因）。
- `failure_recovery` 单次执行日志：每家候选供应商有探测结果（恢复成功 / 跳过及原因 / 探测失败），均含名。
- 后端单测/集成测试覆盖上述日志文本包含供应商名与状态语。
