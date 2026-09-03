# SPEC — 请求日志「供应商」列 + 显示列本地持久化

Label: `ready-for-agent`

## Problem Statement

请求日志表格缺少「供应商」列：每行只显示虚拟模型、API Key、上游模型与耗时/Token 等数值，用户无法直接在行上看到这次请求走了哪个供应商，只能点开详情弹窗。同时需求方期望「显示列」勾选结果能保存在浏览器本地、刷新后保持——该能力（`request-logs:column-visibility` 持久化）已在 main 实现，本期只需把新列纳入其中。

## Solution

`GET /api/request-logs` 列表查询改为 LEFT JOIN provider，返回 `providerName`；前端请求日志表格新增默认可见的「供应商」列渲染该值，缺失时兜底 `#providerId`。新列自动纳入既有列显隐与 localStorage 持久化。

## User Stories

1. 作为管理员，我想在请求日志表格行上直接看到每条请求所用的供应商名称，以便不点开详情就能判断流量走向。
2. 作为管理员，我希望在「显示列」弹窗中看到并可勾选/取消「供应商」列，以便按需控制表格宽度。
3. 作为管理员，我勾选的列显隐状态应保存在浏览器本地，刷新页面后仍按我的选择显示，以便无需每次重新配置。
4. 作为管理员，当某条历史请求对应的供应商已被删除时，表格仍能展示（以 `#id` 兜底），以便不因数据缺失而显示空白。

## Implementation Decisions

### Backend（`src/routes/request_logs.rs`）

- 列表 SQL 由现有 `LEFT JOIN virtual_model vm ...` 增加 `LEFT JOIN provider p ON p.id = r.provider_id`，SELECT 增加 `p.name AS provider_name`。复用 `src/routes/stats.rs:471` 既有 JOIN 先例。
- `RequestLogRow` 结构体增加 `provider_name: Option<String>`（`#[serde(rename_all = "camelCase")]` → `providerName`）。用 `row.try_get(...).ok().flatten()`（同 `virtual_model_display_id` 的容错读取），JOIN 缺失/类型异常不致整行失败。
- 新列不参与排序白名单，不加排序。
- 新增/沿用字段只影响 `GET /api/request-logs` 响应；无 DB 迁移。

### Frontend（`web/src/components/request-logs/RequestLogsTable.tsx`）

- `use-request-logs.ts` 的 `RequestLogRow` 增加 `providerName?: string | null`。
- 表格新增列：

  ```tsx
  {
      accessorKey: "providerName",
      meta: { title: t("requestLogs.provider") },
      header: ({ column }) => (
          <DataTableColumnHeader column={column} title={t("requestLogs.provider")} className={PLAIN_HEADER_CLASS} />
      ),
      cell: ({ row }) => (
          <span className="font-medium">
              {row.original.providerName ?? `#${row.original.providerId}`}
          </span>
      ),
  }
  ```

- 列位：置于「虚拟模型」之后（请求→归属上下文），由列定义顺序决定，与显示列下拉顺序一致。
- 新增列自动纳入 `columnVisibility`/`onColumnVisibilityChange` → `COLUMN_VISIBILITY_KEY` 持久化。**旧 localStorage 只有旧 8 列**：新列不在其中时视为未设置 → 默认可见；用户后续任何勾选会把含新列的完整状态写回。无需迁移既有 localStorage。
- 列头参与排序 UI（`DataTableColumnHeader`），但**未加入后端排序白名单**：手动点排序无意义时保持与其他非排序列一致（前端无禁排特判，若需禁排再议）。

## Testing Decisions

- 后端集成测试（`tests/request_logs_integration.rs`）：
  - 种 provider 行后再种 request，断言返回 `providerName` 为该名称。
  - request 对应 provider 不存在时断言 `providerName` 为 `null`（前端兜底 `#id`）。
- 前端组件测试（`web/src/components/__tests__/request-logs.test.tsx`）：
  - `makeRow` 增加 `providerName`。
  - 断言默认渲染供应商名称。
  - 扩展既有「勾选隐藏列写入 localStorage」用例覆盖「供应商」列。
  - 复用既有键盘打开显示列下拉 + `menuitemcheckbox` 交互模式。
- 全量质量门（见 REQUIREMENTS 验收）。

## Out of Scope

- 重做已存在的列显隐持久化（本期仅纳入新列）。
- 供应商名排序 / 过滤。
- 后端把名称冗余写入 request 表（无迁移）。
- 详情弹窗、其他表格、其他页面。

## Further Notes

- 用户最终选定数据源为后端 JOIN provider 返回名称（见 REQUIREMENTS「方案取舍」）。
- worktree：`feat/request-log-provider-column`（`/Users/ijkzen/Projects/RUST-Project/llm-gateway-provider-column`）。
