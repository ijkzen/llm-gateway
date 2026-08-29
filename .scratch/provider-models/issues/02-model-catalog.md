# 02 — 模型目录模块（vendor + 尾段匹配）

Status: ready-for-agent

- `src/provider_model/data/models.json`：minified vendor（已完成，293KB）。
- `src/provider_model/catalog.rs`：`include_str!` + `OnceLock` 解析为 suffix → 条目索引；serde 结构只取 `limit.context/output`、`reasoning`、`tool_call`、`modalities.input`。
- 匹配函数：两边按 `/` 取最后一段、忽略大小写精确比较；返回 `CatalogEntry { context_length: Option<i64>, max_output_tokens: Option<i64>, reasoning, tool_use, image_understand, video_understand }`。
- 单测：大小写、`models/` 前缀、缺 limit（partial）、未命中、363 条解析成功。

ADR：`docs/adr/0001-embed-model-catalog-as-vendored-asset.md`。

## Comments

- 2026-08-29 完成。后端实现 + 测试全绿（cargo test 102 单测 + 12 新集成测试，clippy 0 警告），并在 4027 端口真实服务冒烟验证（创建/批量/刷新错误透传/级联删除）。
