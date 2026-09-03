# 01: 后端 request-logs 列表 LEFT JOIN provider 返回 providerName

**What to build:** `GET /api/request-logs` 列表查询在现有 `LEFT JOIN virtual_model vm` 之外增加 `LEFT JOIN provider p ON p.id = r.provider_id`，SELECT 增加 `p.name AS provider_name`；`RequestLogRow` 增加 `provider_name: Option<String>`（序列化为 `providerName`），读取容错 `.ok().flatten()`。新列不参与排序白名单。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 后端列表 SQL JOIN 返回 provider_name；provider 缺失时为 null
- [ ] 集成测试：种 provider 行断言返回名称；不种 provider 断言 null
- [ ] `cargo fmt` / `clippy -D warnings` / 相关测试全绿
