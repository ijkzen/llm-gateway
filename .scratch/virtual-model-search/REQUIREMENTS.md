# REQUIREMENTS — 虚拟模型页搜索（对齐供应商模型页规格）

## Scope

为虚拟模型页面（`/virtual-models`）添加与供应商模型页面同规格的搜索：页面标题栏动作区增加搜索框，前端对已加载的虚拟模型成员按 `providerModelId` 做不区分大小写包含匹配，结果按所属虚拟模型分组展示；点击命中行复用页面既有成员详情弹窗；弹窗打开/关闭不丢搜索关键词与结果；点击结果面板外部收起。

## Refined requirements (grilled)

- **匹配对象**：仅匹配成员 `providerModelId`（供应商模型 ID，远端模型名，页面最深一层）；不匹配虚拟模型 `displayId`。
- **结果分组**：按所属虚拟模型分组，组标题为 `displayId`（灰字小标题），组内仅列命中的成员（紧凑行：`providerModelId` + 供应商名灰字），不展开整块卡片。
- **停用实体保留**：停用的虚拟模型、虚拟模型内停用（`item.enable === false`）与随供应商禁用（`providerEnable === false`）的成员均仍出现在搜索结果中；行内不显示状态标记（状态细节由详情弹窗展示）。
- **点击行为**：点击命中行调用页面既有 `setDetail({ virtualModel, item })`，打开 `VirtualModelItemDetailDialog`；弹窗打开/关闭均不清空搜索关键词与结果面板（与供应商模型页 `selectedModel` 抑制点外收起同一模式）。
- **点外收起**：仅当详情弹窗打开或点击落在搜索框/结果面板区域（`searchRef.contains`）时不收起；其余页面区域 pointerdown 收起结果面板。
- **无结果文案**：有搜索关键词但无命中时展示「未找到匹配模型」类文案。

## Non-goals (ponytail cuts)

- 不新增后端端点、数据库字段或迁移；不新增 npm/Rust 依赖。
- 不做服务端全文/模糊/能力搜索；不搜索虚拟模型 `displayId`、供应商名。
- 不做服务端实时远端检索（沿用前端对已加载数据的过滤，与供应商模型页搜索一致）。
- 不新增通用搜索组件、点外检测 hook 或分组面板基础设施；直接复用 `provider-models.tsx` 已实现的 searchRef + pointerdown + 分组 data-testid 模式。
- 供应商模型页同 ticket 中的「供应商启用开关」「添加弹窗 Tab/目录收起」等为供应商页独有内容，不属于本任务，不搬运。

## Open questions resolved by grilling

- 匹配仅限成员 `providerModelId`；结果按所属虚拟模型分组。
- 停用虚拟模型及其停用/随供应商禁用成员保留在结果中，行内无状态标记。
- 点击命中行打开既有成员详情弹窗，搜索会话跨弹窗保留。
- 搜索框位于页面标题栏动作区（参考页同款位置）。
- 纯前端过滤，零后端改动。

## Reference (code facts, explored)

- 规格来源：`.scratch/provider-models-search-and-dialog/`（页面 01 ticket：`issues/01-page-toggle-and-search.md`）。
- 参考实现：`web/src/pages/provider-models.tsx` — `search/searchOpen/searchRef` 状态、`searchGroups` useMemo 按 provider 分组过滤、`document` pointerdown 监听（`selectedModel` 打开或 `searchRef.contains` 时不收起）、结果面板 `data-testid="provider-model-search-results"` / `provider-model-search-group-${id}`。
- 目标页面：`web/src/pages/virtual-models.tsx` — 已持有 `virtualModels`（含 `items: VirtualModelItem[]`）、`detail` 状态 `{ virtualModel, item }` 与 `VirtualModelItemDetailDialog` 渲染。
- 成员字段：`VirtualModelItem.providerModelId / providerName / providerEnable / enable`（`web/src/hooks/use-virtual-models.ts`）。
- 测试基线：`web/src/__tests__/provider-models-page.test.tsx`（搜索/分组/停用可见/详情保持/点外收起断言风格）、`web/src/__tests__/virtual-models-page.test.tsx`（目标页既有测试，detail 弹窗已 mock）。
- i18n 词条对齐：`zh-CN.ts`/`en.ts`（en 用 `satisfies` 保证 key 对齐）。
