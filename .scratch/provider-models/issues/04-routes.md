# 04 — 路由：CRUD + batch + refresh 端点

Status: ready-for-agent

- `src/routes/provider_models.rs`，按 spec 后端 API 表实现 7 个端点；嵌套：providers 路由内 `.nest("/{provider_id}/models", ...)`，全量列表挂 `/api/provider-models`。
- 校验：provider 存在、provider_model_id 非空、两数字 > 0、唯一冲突 400。
- batch：查已存在跳过 + 批内去重（保留首个）+ 事务插入。
- refresh：解密 api_key → 03 的拉取 → 过滤已导入（忽略大小写）→ 目录匹配定三态（smart/partial/manual）→ 候选数组（含已富化字段）。
- 响应结构 camelCase，错误走 `response::bad_request/db_error`。

## Comments

- 2026-08-29 完成。后端实现 + 测试全绿（cargo test 102 单测 + 12 新集成测试，clippy 0 警告），并在 4027 端口真实服务冒烟验证（创建/批量/刷新错误透传/级联删除）。
