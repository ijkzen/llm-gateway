# REQUIREMENTS — 请求日志「供应商」列 + 显示列本地持久化

来源（用户原话）：「请求日志页面，显示列弹窗中添加供应商，并且显示列所选要保存在浏览器本地，刷新页面后读取显示列」

## 现状调查（决定范围）

- **显示列持久化已存在**（commit `d4a5d0f`）：`RequestLogsTable.tsx` 用 `COLUMN_VISIBILITY_KEY = "request-logs:column-visibility"`，挂载时 `loadColumnVisibility()` 读回、`onColumnVisibilityChange` 写回；`PAGE_SIZE_KEY` 同批持久化。**本期不重做。**
- **真正缺口**：请求日志表格**没有「供应商」列**。现有列仅：虚拟模型 / API Key / 上游模型 / 结果 / 输入 / 输出 / 耗时 / 时间。
- 数据：后端 `RequestLogRow` 只有 `provider_id`（数字），不含名称。既有详情弹窗用前端 `useProviderDetail(providerId)` 单独拉名称（request 表不存名称，不新增字段）。

## Scope

1. 后端 `GET /api/request-logs` 列表查询 **LEFT JOIN provider**，为每行返回 `provider_name`（供应商可能已删除，允许 NULL）——复用 `stats.rs` 的既有 JOIN 先例。
2. 前端 `RequestLogsTable.tsx` 新增「供应商」列，渲染 `providerName`；**NULL/缺失兜底 `#${providerId}`**（同详情弹窗语义）。
3. 新列**接入现有列显隐持久化机制**：新列默认可见；用户勾选后写回 localStorage，刷新保持。
4. 详情弹窗不动（已能显示供应商名称）。

## 方案取舍（已拍板）

- **数据源**：后端 JOIN provider 返回名称（用户选定），非前端本地 Map —— 供应商被删/停用仍能按历史 ID 关联出当时名称；删除即不留名时显示 `#id` 兜底。
- provider 行在主库可能缺失（测试与真实删除场景），JOIN 一律 LEFT JOIN，缺名由前端兜底而非后端 COALESCE 空串（与详情「#id」口径一致）。
- 排序：新列**不**加入可排序（供应商名排序无业务价值且不在白名单，零改动）。列显隐下拉里该列默认可见可勾选。
- 不做「默认可见 8 列 + 新列」之外的列序调整；不新迁移；不加排序字段白名单。

## Non-goals（ponytail 修剪）

- 不重做已存在的列显隐 localStorage 持久化。
- 不加供应商名排序。
- 不改详情弹窗 / 过滤项 / 其他页面。
- 不把供应商名称冗余存进 request 表（无迁移）。

## 验收

- 表格出现「供应商」列，展示供应商名称（无名称时 `#id`）。
- 显示列弹窗中「供应商」可勾选/取消，取消后列隐藏。
- 勾选状态写入 localStorage；刷新页面后恢复。
- 集成测试：JOIN 返回 provider_name（含缺 provider 行为）。
- 前端测试：供应商列渲染 + 显隐持久化含该列。
- 质量门：`cargo fmt` / `clippy -D warnings` / `cargo test --all-targets` / `pnpm lint` / `pnpm vitest run` 全绿。
