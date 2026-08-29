# 06 — 前端：导航改名 + hooks + 页面骨架

Status: ready-for-agent

- `web/src/lib/pages.ts`：「模型提供商」→「供应商」；新增「供应商模型」`/provider-models`。
- `web/src/hooks/use-provider-models.ts`：类型（ProviderModel、RefreshCandidate、MatchState）与 hooks（全量列表、按供应商 CRUD/batch、refresh mutation 单独放宽 timeout 30s）。
- `web/src/pages/provider-models.tsx`：每供应商区块（顶行名称+添加按钮、分割线、卡片平铺）、空态引导；路由注册进 App。

## Comments

- 2026-08-29 完成。前端实现 + 测试全绿（vitest 53 用例、biome、tsc、vite build）。期间按用户要求新增 @radix-ui/react-checkbox 依赖与 ui/checkbox.tsx 组件，候选多选用 Checkbox。
