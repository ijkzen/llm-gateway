# 01 — availability 状态迁移日志带供应商名

Type: task
Status: resolved
Blocked by:

## 内容

`src/availability.rs` 三个动作增加 `provider_name: &str` 参数，拼进 info 消息，让「哪家供应商发生状态变更」出现在任务日志 UI（UI 只渲染 message 文本，`provider_id` 结构化字段不可见）：

- `disable_for_quota(db, provider_id, provider_name, label)` → `{label}已耗尽，自动停用供应商「{name}」及其全部虚拟模型子模型（{items} 个）`
- `recover_quota(db, provider_id, provider_name, label)` → `{label}已恢复，自动启用供应商「{name}」及其全部虚拟模型子模型（{items} 个）`
- `recover_probe(db, counters, provider_id, provider_name, expected_updated_at)` → `自动恢复连续失败禁用供应商「{name}」（{items} 个子模型已启用）`

保留 `provider_id`/`items` 结构化字段。`set_items_enabled` 级联日志不动。

## 验收

- 编译期保证新参数存在（调用点全部更新）。
- 单测用 `JobLogLayer` + `with_default` 捕获（`SUBSCRIBER_LOCK` 串行）断言 info message 含 `供应商「{name}」`；现有断言不变。
- 涉及单测：`src/availability.rs::tests`（现有 10 个动作测试改签名）。

## 参考

- spec.md 设计 B
- 调用点：`src/usage/persist.rs::apply_usage_gate`（disable_for_quota/recover_quota）、`src/proxy/failure_recovery.rs::recover_failure_disabled`（recover_probe）
