# 虚拟模型成员只读详情弹窗 + 成员启停开关

## Scope

虚拟模型页面（`web/src/pages/virtual-models.tsx`）中，每个虚拟模型区块平铺的成员卡片（`VirtualModelSection` 里的 `MemberCard`）支持点击：

1. **点击成员卡片 → 打开只读详情弹窗**，展示该成员条目的模型详情。
   - 无论该成员当前是停用（`item.enable === false`）、随供应商禁用（`providerEnable === false`）还是虚拟模型本身停用，均可点击打开。
2. **弹窗样式参考供应商模型详情弹窗**（`ProviderModelDetailDialog` 只读态）：标题为远端模型 ID，描述行「所属供应商：X」，下方 `dl` 展示上下文长度 / 最大输出 / 模型能力（四项能力 支持/不支持 列表），数字采用 `toLocaleString()` 全量格式。
3. **不允许编辑**：无编辑按钮，无删除/测试，纯只读。
4. **弹窗内添加一个启停开关**（「在虚拟模型中启用」）：拨动立即调用 `useUpdateVirtualModel`，把该成员在虚拟模型中的 `enable` 位翻转后提交，生效后 toast 提示，列表数据随 TanStack Query 失效刷新（成员会按排序规则移动位置）。

## Decisions (grilled, user-confirmed)

| # | 决策 | 结论 |
|---|---|---|
| D1 | 开关位置 | 详情弹窗内（推荐采纳） |
| D2 | 弹窗内容 | 仅条目字段 + 状态标记；不展示代理信息（条目响应无代理字段，也不交叉引用） |
| D3 | 供应商停用时开关 | 始终可操作（与编辑弹窗一致），仅展示「随供应商禁用」标记，不锁定开关 |
| D4 | 入口范围 | 仅虚拟模型页面成员卡片；编辑弹窗成员行不加详情入口 |

## Non-goals (ponytail cuts)

- 不做任何后端改动：复用现有 `PUT /virtual-models/{id}` 的 items diff 更新语义（整个成员集合作为最终态提交）。
- 不加代理行、不加模型级网络代理展示。
- 不加编辑/删除/测试按钮（只读弹窗）。
- 编辑弹窗（`VirtualModelEditDialog`）成员行不增加详情入口。
- 不新增 API 端点、不新增依赖。

## Open questions resolved by grilling

- 详情展示哪些字段 → 仅 `VirtualModelItem` 已携带字段 + 状态标记。
- 供应商停用时开关行为 → 仍可操作。
- 入口范围 → 仅页面成员卡片。

## Components touched

- `web/src/components/virtual-models/VirtualModelSection.tsx` — `MemberCard` 由 div 改为可点击（button），新增强制 `onOpen` 回调。
- `web/src/components/virtual-models/VirtualModelItemDetailDialog.tsx`（新建）— 只读详情 + 启停开关。
- `web/src/pages/virtual-models.tsx` — 持有选中状态，渲染详情弹窗。
- `web/src/i18n/locales/zh-CN.ts` / `en.ts` — 新增开关标签等少量词条。
- 测试：`web/src/__tests__/virtual-models-page.test.tsx`（点击卡片开弹窗）、`web/src/components/__tests__/virtual-models-dialogs.test.tsx`（弹窗展示 + 开关提交）。