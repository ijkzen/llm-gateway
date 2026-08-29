# 01 — entity provider_model + 迁移 + 级联删除

Status: ready-for-agent

- 新建 `src/entity/provider_model.rs`（列结构与默认值见 spec 数据层），注册进 `src/entity/mod.rs`。
- `src/db.rs::migrate()` 增加 entity 建表块；migration 5 建 `idx_provider_models_provider_id` 索引。
- `src/routes/providers.rs::delete_provider` 改为事务内先删 `provider_model where provider_id = ?` 再删 provider（应用层级联硬删）。

## Comments

- 2026-08-29 完成。后端实现 + 测试全绿（cargo test 102 单测 + 12 新集成测试，clippy 0 警告），并在 4027 端口真实服务冒烟验证（创建/批量/刷新错误透传/级联删除）。
