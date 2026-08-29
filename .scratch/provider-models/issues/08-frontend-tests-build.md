# 08 — 前端测试与构建

Status: ready-for-agent

- vitest：卡片能力图标、详情弹窗编辑态切换、添加弹窗三态与解锁逻辑、手动添加表单校验。
- `pnpm lint`、`pnpm vitest run`、`pnpm build` 全绿（build 产物供 rust-embed）。

## Comments

- 2026-08-29 完成。前端实现 + 测试全绿（vitest 53 用例、biome、tsc、vite build）。期间按用户要求新增 @radix-ui/react-checkbox 依赖与 ui/checkbox.tsx 组件，候选多选用 Checkbox。
