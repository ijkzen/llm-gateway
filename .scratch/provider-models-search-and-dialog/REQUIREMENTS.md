# REQUIREMENTS — 供应商模型：供应商开关、搜索与添加弹窗

## Scope

调整供应商模型页面（`/provider-models`）：为每个供应商提供启用开关；增加按供应商模型 ID 搜索并按供应商分组展示结果；重构添加供应商模型弹窗为自动添加与手动添加两个 Tab，并修复手动添加目录匹配候选的点外收起行为。

## Refined requirements (grilled)

- **供应商开关**：每个供应商区块标题行的「添加」按钮左侧放置开关。切换直接更新该供应商的 `enable` 状态；请求失败时回滚并提示错误，成功后刷新供应商数据。
- **搜索位置与匹配**：页面标题栏右侧增加搜索框，匹配已登记 `ProviderModel.providerModelId`。使用已加载的供应商与供应商模型数据进行前端筛选，不新建搜索接口。
- **搜索结果**：按所属供应商分组展示，禁用供应商及其模型仍包含在内。点击结果复用现有供应商模型详情弹窗；详情弹窗打开、关闭时均保留搜索关键词与结果。仅在搜索结果区域外点击时收起结果面板。
- **添加弹窗 Tab**：弹窗提供「自动添加」和「手动添加」两个 Tab；前者承载刷新候选、多选导入流程，后者承载单模型表单和目录联想。
- **添加弹窗布局**：弹窗采用适配视口的固定高度；标题区域和底部操作区位置、高度固定，中间内容区填满余下空间并独立滚动。
- **手动目录联想**：输入模型名称展示的模型目录候选，在点选候选（填充表单）或点击候选列表外时收起。

## Non-goals (ponytail cuts)

- **不增加后端端点或数据库字段**：供应商开关复用现有供应商更新接口；搜索只针对当前已加载的登记模型。
- **不做服务端全文、模糊或能力搜索**：只按 `providerModelId` 匹配；供应商名称仅用作分组标题。
- **不调整禁用语义或级联状态**：本次仅编辑 `Provider.enable`，沿用现有选路和可用性逻辑。
- **不新增通用搜索、弹窗或点击外部检测基础设施**：优先复用已有 React、Radix 与 shadcn 能力，在当前页面/弹窗内完成。
- **不增加 npm 或 Rust 依赖，不新增 ADR/领域词条**：现有 Provider、ProviderModel 与模型目录术语足以覆盖本次界面调整。

## Open questions resolved by grilling

- 搜索仅匹配供应商模型 ID，结果按供应商分组。
- 禁用供应商的模型仍应出现在管理页搜索中。
- 点击搜索结果后，关键词和结果在详情弹窗关闭后继续保留；点击结果区域外才收起。
- 搜索框位于页面标题栏右侧。
- 开关立即切换；失败回滚。
- 手动目录匹配候选在点选或点外时收起。

## Reference (code facts, explored)

- 页面与详情状态：`web/src/pages/provider-models.tsx` 已持有 `selectedModel` 并渲染 `ProviderModelDetailDialog`。
- 供应商模型区块：`web/src/components/provider-models/ProviderModelSection.tsx` 当前标题行仅有「添加」按钮。
- Provider 已有 `enable` 字段；`useUpdateProvider()` 的现有调用已使用「即时显示、失败回滚」模式。
- 页面已有全量 `useProviders()` 与 `useProviderModels()` 查询，`ProviderModel.providerId` 可关联分组。
- 添加弹窗：`web/src/components/provider-models/AddProviderModelsDialog.tsx` 已包含刷新候选、手动表单和 `useCatalogSearch()` 联想数据。
