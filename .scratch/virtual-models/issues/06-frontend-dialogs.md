# 06 — 前端弹窗：创建/编辑与详情

Status: ready-for-agent

## 任务

- `VirtualModelEditDialog.tsx`（创建/编辑共用）：displayId Input → 两个策略 Select → 分割线 → 按供应商分组的成员多选（Checkbox 网格 + 上下文长度；禁用供应商带标记仍可选）；**互斥排除**（mappedModelIds 由页面从列表数据推导，编辑时保留自身成员）；「已选 N / 共 M 个」；未选禁用提交；编辑提交保留未变成员 enable、新增成员缺省 enable。
- `VirtualModelDetailDialog.tsx`：只读策略 + 成员列表（供应商名/远端 ID/能力图标/启停 Switch/「随供应商禁用」标记）；头部虚拟模型启停 Switch；底部删除（ConfirmDialog）+ 编辑（页面切换弹窗）。
- 已映射集合排除不需要新接口：由页面从 `useVirtualModels` 列表数据计算。

## Comments

2026-08-29 完成。删除确认内嵌在详情弹窗（对齐 ProviderModelDetailDialog 模式），不再单设页面级删除弹窗；成员启停/虚拟模型启停均通过 PUT diff 语义提交。

2026-08-29 修订（用户反馈交互不符）：废弃「小卡片网格 + 详情弹窗」，重构为区块式——
- 新增 `VirtualModelSection`（顶行 display_id + 策略 badge + 「⋯」菜单[编辑/删除] + 分割线 + 平铺纯展示成员卡片）与 `VirtualModelDeleteDialog`（页面级二次确认）；删除 `VirtualModelCard`/`VirtualModelDetailDialog`，能力图标抽为共享 `ItemCapabilityIcons`。
- `VirtualModelEditDialog` 重写为暂存模式：顶部基本信息（display_id/两策略/**虚拟模型启用开关**），下方按供应商分组管理成员——组「添加」展开候选区（排除其他虚拟模型占用与已加入暂存的，点击即入暂存），成员行启停 Switch + 移除按钮；「保存/创建」一次性提交全量 items（后端 diff 语义不变）。
- 决策（AskUserQuestion 确认）：弹窗内操作暂存保存一次性生效；启用开关放编辑弹窗顶部；区块成员卡片纯展示不可点击。
- Edge 浏览器走查通过：区块布局、菜单、编辑回填、暂存保存写库（o3 enable false→true）均符合预期。
