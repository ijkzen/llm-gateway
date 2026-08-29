# 07 — 前端：卡片/详情弹窗/添加大弹窗

Status: ready-for-agent

- `web/src/components/provider-models/`：
  - `ProviderModelCard`：模型 ID + 能力图标（仅 true 的，tooltip）。
  - `ProviderModelDetailDialog`：只读默认；「编辑」→ 编辑态右上「删除」+「更新」；更新后回只读；关闭即丢弃。
  - `AddProviderModelsDialog`：「尝试刷新」+ 候选卡片（勾选框不预选、绿/黄/需手动填写三态、内联补数字解锁勾选）+ 底部「添加」批量导入 + 常驻「手动添加」表单。
- 风格对齐 nyro 玻璃拟态，弹窗复用现有 Dialog/Form 模式。

## Comments

- 2026-08-29 完成。前端实现 + 测试全绿（vitest 53 用例、biome、tsc、vite build）。期间按用户要求新增 @radix-ui/react-checkbox 依赖与 ui/checkbox.tsx 组件，候选多选用 Checkbox。
