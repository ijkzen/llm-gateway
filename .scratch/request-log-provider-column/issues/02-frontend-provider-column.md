# 02: 前端请求日志表新增「供应商」列并接入列显隐持久化

**What to build:** `web/src/hooks/use-request-logs.ts` 的 `RequestLogRow` 增加 `providerName?: string | null`；`web/src/components/request-logs/RequestLogsTable.tsx` 在「虚拟模型」列后新增 accessorKey `providerName` 的「供应商」列，`meta.title` 用 `requestLogs.provider`（i18n 已有），cell 渲染 `providerName ?? #providerId`（font-medium，同虚拟模型列视觉）。新增列自动纳入既有 `columnVisibility`/`COLUMN_VISIBILITY_KEY` localStorage 持久化，无需迁移旧数据（缺省默认可见）。

**Blocked by:** 01 (后端返回 providerName)

**Status:** ready-for-agent

- [ ] 组件测试 `makeRow` 增加 providerName；默认渲染供应商名
- [ ] 扩展列显隐持久化用例覆盖「供应商」列
- [ ] `pnpm lint` / `pnpm vitest run` 全绿
