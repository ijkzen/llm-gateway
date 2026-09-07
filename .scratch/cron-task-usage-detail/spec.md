# Spec — 定时任务明细日志（供应商粒度）

Feature slug: `cron-task-usage-detail` · Status: ready-for-agent · Date: 2026-09-07

## 背景

两个内置定时任务 handler 目前只输出聚合计数：

- `usage_refresh`（`@every 5m`）：`用量刷新完成，成功刷新 {n} 家供应商`
- `failure_recovery`（`@hourly`）：`连续失败供应商恢复完成，成功恢复 {n} 家供应商`

状态变更虽在 `src/availability.rs` 记了 `info!(provider_id, items, ...)`，但 `provider_id` 是结构化字段，**任务日志 UI（`src/cron/log_capture.rs` 只渲染 message 文本）看不到**。需求：把「哪家供应商发生了什么状态变更」写进任务执行日志，不要只写数字。

## 目标

1. `usage_refresh` 执行日志：**变化 + 失败点名**（用户拍板）。
   - 刷新失败：`warn` 逐条点名（供应商名 + 错误）。
   - 额度停用/恢复（quota 态迁移）：`info` 点名（供应商名 + 订阅额度/余额 + 触发窗口/余额原因 + 级联子模型数）。
   - 刷新成功：不逐条；收尾保留 `成功刷新 {n} 家供应商` 汇总行。
2. `failure_recovery` 执行日志：**每家候选逐条点名**（每小时、候选少）：
   - 恢复成功：`info` 点名（供应商名 + 级联子模型数）。
   - 用量门控拦截 / 无模型 / 无 Key / 探测失败 / 状态更新失败：`warn` 逐条点名（供应商名 + 原因）。
   - 收尾保留 `成功恢复 {n} 家供应商` 汇总行。

## 非目标

- 不新增数据库列/迁移/前端改动。
- 不改 `disable_for_failures`（转发链路熔断，非定时任务路径）与手动启停日志。
- 不改 `refresh_all_usage` / `recover_failure_disabled` 的返回签名与调用方。

## 设计

### A. 消息命名约定

所有受影响日志消息以「供应商」为表述单位：

```
供应商「{name}」{发生了什么}
```

结构化字段同步携带 `provider_id` 与 `provider_name`（tracing 字段进 JSON 文件日志；UI 只看 message）。避免裸 id、避免只写数字。

### B. `src/availability.rs` —— 状态迁移日志带名

给三个动作增加 `provider_name: &str` 参数，仅用于拼日志消息（消息为格式化字符串，provider_name 不单独成字段）：

- `disable_for_quota(db, provider_id, provider_name, label)`：`{label}已耗尽，自动停用供应商「{name}」及其全部虚拟模型子模型（{items} 个）`
- `recover_quota(db, provider_id, provider_name, label)`：`{label}已恢复，自动启用供应商「{name}」及其全部虚拟模型子模型（{items} 个）`
- `recover_probe(db, counters, provider_id, provider_name, expected_updated_at)`：`自动恢复连续失败禁用供应商「{name}」（{items} 个子模型已启用）`

参数放最后、统一 `&str`。保留 `provider_id`/`items` 结构化字段。`set_items_enabled` 的级联日志保持现状（其中 count=0 时不上日志）。

### C. `src/usage/persist.rs` —— refresh_all_usage 失败点名

`refresh_all_usage` 成功路径保持静默（汇总行在 handler 层）；**失败路径** `warn` 改为带名：

```
tracing::warn!(provider_id = p.id, provider_name = &p.name, "供应商「{name}」用量刷新失败：{e}");
```

新增一个内部 helper 输出「发生状态变更」的逐条行（若采用集中汇总方案）——见「实现缝」C1。`apply_usage_gate` 保持把 `p.name` 传入 availability 动作。

### D. `src/proxy/failure_recovery.rs` —— 逐条点名

`recover_failure_disabled` 内所有 `warn` 消息补 `供应商「{name}」` 前缀 + `provider_name` 字段；对「用量门控拦截（`usage_allows_probe` 为 false）」补一条带名说明；`recover_probe` 成功与否的带名信息已由 availability（B）负责；`Ok(true)` 分支计数。

### E. `src/lib.rs` —— handler 汇总行与重叠跳过

汇总行保持数字但补上语义边界（若各轮次行已在 C/D 输出，汇总行无需再点名）：
- `用量刷新完成：成功刷新 {n} 家供应商用量`（n=0 也输出，替换当前静默/return 早退的歧义）
- `连续失败供应商恢复完成：成功恢复 {n} 家供应商`

「上次仍在运行，本次跳过」行已带任务语义，维持现状。

## 实现缝

- **C1**：状态变更「逐条点名」由 availability 状态机（B）在变更发生时直接输出 —— 变更点天然在各 availability 动作内部，符合 ADR-0003「动作自带日志」。无需在 refresh_all_usage 里二次收集/汇总。刷新失败逐条行在 refresh_all_usage 失败分支输出。
- **D1**：`usage_allows_probe` 返回值丢弃位置需要记录「因用量不可用而拦截」——在 `recover_failure_disabled` 调用处补分支日志。
- **E1**：`refresh_all_usage` 对 targets 为空时返回 Ok(0)，handler 仍输出汇总行（当前 handler 在 Ok(0) 也输出），语义为「0 家目标」而非「任务失败」。

## 测试

### T1 availability 单测（`src/availability.rs::tests`）
- 新参数 `provider_name` 存在性由编译保证。断言日志消息含名字：用 `tracing_test` 或现有 SUBSCRIBER 测试方式（`log_capture` 测试用 `tracing::subscriber::with_default` + `JobLogLayer` 捕获），断言 message 含 `供应商「p1」`。
- 现有单测全部改为新签名（调用点 + 断言不变）。

### T2 `src/usage/persist.rs` 单测
- `refresh_all_usage` 失败路径：注入失败 provider，捕获日志断言含 `供应商「{name}」用量刷新失败`。用 SUBSCRIBER 锁串行。
- 现有单测签名调整。

### T3 `tests/provider_quota_gate_integration.rs`
- `apply_usage_gate` 停用/恢复断言不变；可增加日志断言（可选，用 log_capture 捕获；如成本高则仅依赖单测）。

### T4 `tests/provider_failure_recovery_integration.rs`
- 现有对 `recover_failure_disabled` 返回值断言不变；增加/确认日志点名断言（探测失败、无模型、无 Key、用量拦截路径各一条）。

## 开放问题 / 后续（不阻塞）

- 是否需要把「供应商名」也补进 `disable_for_failures`（转发链路）日志 —— 超出本 spec 范围，另开票。

## 参考文件

- 需求：`.scratch/cron-task-usage-detail/REQUIREMENTS.md`
- 涉及源文件：`src/availability.rs`、`src/usage/persist.rs`、`src/proxy/failure_recovery.rs`、`src/lib.rs`
- 相关测试：`src/availability.rs::tests`、`src/usage/persist.rs::tests`、`tests/provider_quota_gate_integration.rs`、`tests/provider_failure_recovery_integration.rs`

## 评审后决策记录（2026-09-07，code-review + ponytail-review）

- **修复**：`refresh_all_usage` 内「用量额度门控执行失败」warn 补供应商名（原裸 provider_id）。
- **修复**：`failure_recovery` 用量门控改为三态 `ProbeGate`（Allowed/Blocked/UsageUnusable），消除「用量查询失败」与「用量不可用」双记日志与误标；查询失败只在上层记一条，确定性不可用才点名「用量不可用，跳过」。
- **豁免（E 措辞不应用）**：spec 设计 E 提议的汇总行措辞调整（`用量刷新完成：成功刷新 {n} 家供应商用量`）不实施——原措辞已含计数语义，n=0 行为本就成立，改动纯修饰无用户价值。
- **豁免（T4 全分支不补测）**：探测失败/无 Key/用量门控拦截等消息断言需真实 HTTP mock + 捕获 harness（集成测试无 subscriber），模板字符串无逻辑，代表性路径已由三个捕获测试覆盖；不为此加重型测试基建。
- **ponytail-review**：Lean already. Ship. —— 测试仅覆盖消息路径、`ProbeGate` 因修复真实缺陷而存在，无冗余抽象。
