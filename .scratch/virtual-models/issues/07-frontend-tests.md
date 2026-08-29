# 07 — 前端测试与回归

Status: ready-for-agent

## 任务

- `web/src/__tests__/virtual-models-page.test.tsx`：骨架屏、错误态、空态 + 添加按钮开弹窗（props 捕获）、卡片平铺与详情联动。
- `web/src/components/__tests__/virtual-models-dialogs.test.tsx`：创建模式互斥排除/未选禁用/提交 payload、编辑模式回填与 enable 保留、新成员不带 enable、空 displayId 不提交；详情弹窗策略与成员渲染（随供应商禁用标记）、成员启停/虚拟模型启停提交 payload、删除确认。
- `pnpm lint`、`pnpm vitest run`、`pnpm build` 全绿。

## Comments

2026-08-29 完成。新增 9 个前端用例（20 文件 65 用例）全部通过；lint 全绿（顺带修复 ProviderModelDetailDialog.tsx 的既有 format 漂移）；`pnpm build` 通过。

2026-08-29 修订：随交互重构重写两个测试文件——页面测试覆盖区块渲染/停用标记/菜单开弹窗（Radix DropdownMenu 在 jsdom 用 keyDown Enter 打开）；弹窗测试覆盖暂存增删启停/两供应商汇总 payload/互斥候选排除/enable 保留/删除二次确认。现有 20 文件 67 用例全绿；lint、build 全绿。
