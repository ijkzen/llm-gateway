# 03 — OpenAI 兼容接口 /v1/models

Status: ready-for-agent

## 任务

- 新建 `src/routes/openai_compat.rs`：`GET /v1/models`、`GET /v1/models/{display_id}`，`src/routes/mod.rs` 挂载 `.nest("/v1", ...)`。
- 列表：只含 `enable = true` 的虚拟模型，返回 `{"object":"list","data":[{"id","object":"model","created","owned_by":"llm-gateway"}]}`；serde_json 直出。
- 详情：同结构单对象；不存在或已禁用 → 404 `{"error":{"message","type":"invalid_request_error","code":"model_not_found"}}`。
- 暂不鉴权（与现状一致），网关鉴权属后续需求。

## Comments

2026-08-29 完成。标准 OpenAI 格式（用户确认不附扩展字段）；`created` 取虚拟模型 `created_at` 的 unix 秒。
