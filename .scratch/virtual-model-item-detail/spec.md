---
feature: virtual-model-item-detail
status: ready-for-agent
---

# 虚拟模型成员只读详情弹窗 + 成员启停开关

## Problem Statement

虚拟模型页面把每个虚拟模型及其成员模型以「区块 + 平铺卡片」展示，但成员卡片只是静态展示，无法查看某个成员模型的完整详情（上下文长度、最大输出、能力项等）。用户想快速查看成员详情并就地调整该成员在虚拟模型中的启停状态，而不必进入「编辑」弹窗。

## Solution

在虚拟模型页面，每个成员卡片变为可点击：点击后弹出「成员模型详情」只读弹窗（样式对齐供应商模型详情弹窗的只读态），展示该成员所指向供应商模型的模型 ID、所属供应商、上下文长度、最大输出、四项模型能力及当前状态标记；弹窗内提供一个「在虚拟模型中启用」开关，拨动立即持久化该成员在虚拟模型内的启用位。弹窗不允许编辑（无编辑/删除/测试按钮）。

## User Stories

1. 作为管理后台用户，我希望点击虚拟模型区块里的成员卡片能打开一个详情弹窗，以便查看该模型的上下文长度、最大输出与能力项，不用进编辑弹窗。
2. 作为管理后台用户，我希望即使该成员已停用、随供应商禁用或所在虚拟模型已停用，卡片仍然可以点击打开详情，以便了解该成员的配置。
3. 作为管理后台用户，我希望详情弹窗是只读的（没有编辑按钮），以便快速浏览而不会误触修改。
4. 作为管理后台用户，我希望详情弹窗里的启停开关能直接改变该成员在虚拟模型中的启用状态并立即生效，以便无需进入编辑弹窗即可微调成员启停。
5. 作为管理后台用户，我希望开关失败时有明确错误提示，以便知道操作未生效。
6. 作为管理后台用户，我希望弹窗展示「随供应商禁用」「已停用」等状态标记，以便在详情里清楚看到该成员当前为何不可用（供应商级别 vs 虚拟模型内级别）。
7. 作为开发者，我希望该功能复用以 `PUT /virtual-models/{id}` 的现有 items diff 更新语义，以便不新增后端端点。

## Implementation Decisions

- 仅前端改动，零后端改动。启停操作复用 `useUpdateVirtualModel`，把当前虚拟模型成员集合整体提交，仅翻转目标成员的 `enable`（后端按集合 diff 增量更新并清除 `cascade_disabled` 手动接管）。
- 新建 `web/src/components/virtual-models/VirtualModelItemDetailDialog.tsx`：`open`/`onOpenChange`/`virtualModel`/`item` props；只读展示区样式参照 `ProviderModelDetailDialog`（标题=远端模型 ID、描述=「所属供应商：」+供应商名、`dl` 行展示上下文长度/最大输出/能力；数字用 `toLocaleString()` 全量格式）；底部一行「在虚拟模型中启用」开关。
- 弹窗内容仅取 `VirtualModelItem` 自带字段 + 状态标记，不交叉引用 `ProviderModel` 代理字段（条目响应无代理字段）。
- 供应商停用（`item.providerEnable === false`）时开关仍然可操作，仅展示「随供应商禁用」标记（与 `VirtualModelEditDialog` 成员行行为一致）；成员 `item.enable === false` 展示「已停用」标记。
- `VirtualModelSection` 的 `MemberCard` 改为可点击（button 语义，保留现有禁用样式 opacity），并通过新增 `onOpen` 回调上抛到页面；`virtual-models.tsx` 持有选中项状态并渲染详情弹窗。编辑弹窗（`VirtualModelEditDialog`）成员行不加详情入口。
- 开关 pending 时禁用；成功 toast「操作成功」并依赖 TanStack Query 失效刷新（成员排序会随之重排）；失败 toast 报错并保持原状态。
- i18n：复用 `providerModels.belongsToProvider/contextLength/maxOutput/modelCapabilities/supported/notSupported`、`virtualModels.disabledWithProvider/disabledMark`、`common.*`；新增开关标签词条（如 `virtualModels.enableInVirtualModel`）及必要的标题/描述词条，zh-CN 与 en 同步对齐（`en.ts` 用 `satisfies` 保证 key 对齐）。

## Testing Decisions

- 只测外部行为，不测实现细节。
- **接缝 1（弹窗组件）**：`web/src/components/__tests__/virtual-models-dialogs.test.tsx` 扩展——渲染 `VirtualModelItemDetailDialog`，断言只读展示（模型 ID、供应商、上下文/最大输出、能力、状态标记、无编辑/删除按钮）+ 开关拨动后 `useUpdateVirtualModel` 收到含目标成员 `enable` 翻转的完整 items 载荷；供应商停用时开关仍可交互（用 `toHaveBeenCalled` 校验）。
- **接缝 2（页面点击打开）**：`web/src/__tests__/virtual-models-page.test.tsx` 扩展——mock 详情弹窗，分别以「正常成员 / 已停用成员 / 随供应商禁用成员 / 虚拟模型停用」四种成员状态点击卡片，断言详情弹窗 `open` 置真。
- 前置参照：`web/src/components/__tests__/provider-models-dialogs.test.tsx`（只读展示与开关交互）、`web/src/__tests__/virtual-models-page.test.tsx`（现有卡片渲染与菜单交互）。
- 前端门禁：`pnpm lint`（Biome）+ `pnpm vitest run` 全绿。

## Out of Scope

- 任何后端改动（不改路由/实体/迁移）。
- 弹窗内编辑、删除、测试能力；代理信息展示。
- 编辑弹窗（`VirtualModelEditDialog`）成员行增加详情入口。
- 独立 helper 单元测试文件。

## Further Notes

- 成员排序由后端 `sort_items` 决定（启用优先 → LB 策略分组 → 字母序），开关关闭后成员会沉到区块后方，属预期行为，测试中不断言位置。
- 详情弹窗内容与供应商模型详情弹窗的差异点（无代理行、无测试/编辑/删除按钮）是基于条目自身字段边界与「仅只读」要求的主动裁剪。
- 工作分支：worktree `/Users/ijkzen/Projects/RUST-Project/llm-gateway-virtual-model-item-detail`，分支 `fix/virtual-model-item-detail`。