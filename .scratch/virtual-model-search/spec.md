# Spec — 虚拟模型页搜索（对齐供应商模型页规格）

Feature: virtual-model-search
Status: ready-for-agent

## Problem Statement

虚拟模型页面把每个虚拟模型及其成员模型以「区块 + 平铺卡片」展示。登记的虚拟模型变多后，管理员无法快速定位某个上游模型（`providerModelId`）被哪个或哪些虚拟模型引用，只能逐区块目视扫描成员卡片。供应商模型页面已有同款问题的解决方案：标题栏搜索框 + 按上层分组的临时检索结果面板 + 点击结果打开既有详情弹窗且保留检索会话。

## Solution

在虚拟模型页面（`/virtual-models`）标题栏动作区提供与供应商模型页同规格的搜索：输入 `providerModelId` 关键词后，前端对页面已加载的虚拟模型成员做不区分大小写的包含匹配，命中结果按所属虚拟模型分组显示在搜索框下方的浮层面板中；停用的虚拟模型及其停用/随供应商禁用成员仍出现在结果中（便于核查或重新启用前查看）。点击命中行打开该成员既有的「成员模型详情」弹窗（传入所属虚拟模型与命中成员），弹窗打开与关闭均保留搜索关键词与结果面板；点击结果面板以外区域收起面板。纯前端改动，零后端改动。

## User Stories

1. 作为管理后台用户，我希望在虚拟模型页标题栏输入关键词，即可按供应商模型 ID（成员 `providerModelId`）检索，以便从大量虚拟模型及其成员中快速定位某个上游模型被谁引用。
2. 作为管理后台用户，我希望搜索匹配不区分大小写、按包含子串匹配，以便检索规则稳定、可预期（与供应商模型页一致）。
3. 作为管理后台用户，我希望命中结果按所属虚拟模型分组、组标题显示虚拟模型 ID（`displayId`），以便看清同一上游模型被哪些虚拟模型聚合。
4. 作为管理后台用户，我希望分组内只列出命中的成员（每行显示 `providerModelId` 与所属供应商名），以便结果浮层保持紧凑、专注命中项。
5. 作为管理后台用户，我希望停用虚拟模型中的命中成员仍出现在结果中，以便核查配置或重新启用前能定位到它。
6. 作为管理后台用户，我希望虚拟模型内已停用、随供应商禁用的命中成员仍出现在结果中，以便了解它的配置与状态。
7. 作为管理后台用户，我希望点击命中行直接打开该成员的既有详情弹窗，以便不必先在长页面中寻找卡片。
8. 作为管理后台用户，我希望在打开和关闭详情弹窗后继续保留搜索关键词与结果面板，以便连续检查多个命中的成员。
9. 作为管理后台用户，我希望点击结果面板以外的页面区域时收起面板，以便继续浏览页面时消除临时遮挡。
10. 作为管理后台用户，我希望关键词无命中时看到明确的无结果提示，以便知道不是搜索失效。

## Implementation Decisions

- 仅在虚拟模型列表页（`/virtual-models`）实现；搜索为纯前端过滤，作用域为页面已加载的 `useVirtualModels()` 数据，不请求新 API、不检索未加载内容。匹配键为成员 `providerModelId`（不区分大小写包含）；不匹配虚拟模型 `displayId` 或供应商名。
- 页面新增 `search` / `searchOpen` / `searchRef` 状态，结构照搬 `web/src/pages/provider-models.tsx` 已实现的搜索块模式（不抽取通用组件）：搜索框位于标题栏动作区（刷新/添加按钮旁）；命中分组经 `useMemo` 派生；`document` `pointerdown` 监听收起——当详情弹窗打开（选中成员态非空）或点击落在 `searchRef` 区域内时不收起，其余点击收起。
- 结果面板为搜索框下方的绝对定位浮层：按页面既有虚拟模型排列顺序分组（组内仅命中成员），组标题 `displayId` 小字灰字，行内 `providerModelId` + `providerName` 小字灰字，无状态标记；面板沿用参考实现的 `data-testid`（结果容器 + 分组）便于测试；空命中展示无结果文案。
- 点击命中行设置页面既有的 `detail` 状态 `{ virtualModel, item }`，复用已渲染的 `VirtualModelItemDetailDialog`（不另建详情实现）。「详情弹窗打开时不收起结果面板」通过选中态非空抑制点外收起实现，与参考实现 `selectedModel` 同模式。
- i18n：新增虚拟模型搜索专属词条（placeholder/aria-label「搜索虚拟模型成员」或对齐语义、无结果文案），`zh-CN.ts` 与 `en.ts` 同步（en 侧 `satisfies` 保持 key 对齐）；复用 `common.refresh` 等既有词条。

## Testing Decisions

- 只测用户可观察行为（搜索框输入、结果面板出现/分组、详情弹窗打开、点击外部收起），不测组件内部状态或 CSS 实现。
- 唯一 seam：既有页面测试 `web/src/__tests__/virtual-models-page.test.tsx`（该文件已 mock `VirtualModelItemDetailDialog` 并在 mock 上记录 `open`/`item`/`virtualModel`，直接承载交互断言）；前置参照 `web/src/__tests__/provider-models-page.test.tsx` 的搜索用例（按 ID 搜索、分组 `data-testid`、停用可见、点结果开详情、详情后关键词保留、点外收起）。
- 用例覆盖：按 `providerModelId` 搜索并命中跨多个虚拟模型的成员、按所属虚拟模型分组、停用虚拟模型与停用成员的命中可见、点结果行打开详情弹窗且传入正确成员、详情关闭后关键词与结果保留、点击结果面板外部收起、无结果文案。
- 纯前端 UI 行为，不新增 Rust 测试；门禁 `pnpm lint`（Biome）+ `pnpm vitest run` 全绿。

## Out of Scope

- 任何后端改动（不改路由/实体/迁移/搜索接口）；不新增 npm/Rust 依赖。
- 匹配虚拟模型 `displayId`、供应商名或模型能力的搜索；服务端全文/模糊/实时检索。
- 结果面板内展示整块成员卡片、状态标记或启停操作；不改变成员/虚拟模型/供应商的任何领域语义。
- 供应商模型页同 ticket 的供应商启用开关、添加弹窗 Tab 与目录联想（供应商页独有，不搬运）。
- 抽取通用搜索/点外检测组件或 hook（直接复用参考实现模式）。

## Further Notes

- 术语：虚拟模型（VirtualModel，对外模型聚合，`displayId` 为其 ID）；虚拟模型成员（VirtualModelItem，指向一个供应商模型条目，`providerModelId` 为远端模型 ID）；供应商模型页为规格来源页面。
- 测试切面已由需求会话确认：单 seam（页面级交互测试），行为契约 6 条全部落入该 seam。
- 参考先例：`.scratch/provider-models-search-and-dialog/spec.md`（规格）、`web/src/pages/provider-models.tsx`（实现模式）、`web/src/__tests__/provider-models-page.test.tsx`（测试断言风格）。
