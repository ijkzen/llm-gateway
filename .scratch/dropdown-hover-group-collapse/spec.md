# Spec — 浮窗列表项 hover 修复 + 分组折叠

位置：`.scratch/dropdown-hover-group-collapse/spec.md`
状态：ready-for-agent
日期：2026-09-07

## Problem Statement

1. 供应商/供应商模型等**弹窗内**的下拉浮窗，鼠标悬停选项时**没有高亮背景**，用户难以预判点击目标。根因是 Radix Dialog（modal）的 focus-trap：Select/Dropdown 浮层 portal 到 body、位于 Dialog 焦点域外，选项无法取得焦点，`data-highlighted` 永不置位，依赖 `focus:bg-accent` 的高亮永不触发（已 CDP 实证；页面内非 Dialog 下拉正常，对照可复现）。
2. 若干「按供应商/按虚拟模型分组」显示的浮窗列表（多选过滤、搜索联想）没有分组折叠能力，分组很多时浏览/检索不便。chat 模型选择器已支持折叠，其余未支持。

## Solution

1. 给 Radix Select/Dropdown 的可点列表项补 **CSS `:hover` 背景**（不依赖 focus），弹窗内外统一可见。
2. 给分组浮窗列表加分组的**折叠/展开**（默认全展开、会话内 useState、标题行可点 + Chevron）。

## User Stories

1. 作为管理员，我在「新建/编辑供应商」弹窗里选协议类型、付费类型时，希望鼠标划过各选项能看到高亮背景，以便确认即将选中的项。
2. 作为管理员，我在供应商模型详情弹窗里选协议（跟随供应商/覆盖）时，希望同样的 hover 高亮可见。
3. 作为管理员，我在虚拟模型编辑弹窗里选接口类型、负载策略、回退策略时，希望同样的 hover 高亮可见。
4. 作为管理员，我在请求日志页过滤上游模型（按供应商分组的多选下拉）时，希望展开的下拉能按供应商折叠/展开，收起暂时不需要的供应商组。
5. 作为管理员，我在请求日志页过滤供应商/虚拟模型（无分组的多选下拉）时，希望列表行为与现状一致、不出现多余的折叠控件。
6. 作为管理员，我在供应商模型页顶部搜索模型（结果按供应商分组）时，希望可折叠暂时不看的分组。
7. 作为管理员，我在虚拟模型页顶部搜索成员（结果按虚拟模型分组）时，希望可折叠暂时不看的分组。
8. 作为管理员，我在聊天页选择模型（按供应商分组、已支持折叠）时，希望行为保持不变。
9. 作为管理员，我用键盘导航这些下拉/分组浮窗时，希望既有 focus 高亮与 `aria-expanded` 等可访问性语义不受影响。

## Implementation Decisions

### D1 — Radix 项 hover 背景（纯 CSS 类，ui 原语层统一修）

修改 `web/src/components/ui/` 两个原语文件，仅追加 Tailwind 类，不动结构、不动现有 `focus:*`：

- `select.tsx` → `SelectItem`：类串中 `focus:bg-accent focus:text-accent-foreground` 前追加 `hover:bg-accent hover:text-accent-foreground`。
- `dropdown-menu.tsx`：
  - `DropdownMenuItem`（含 destructive variant）：追加 `hover:bg-accent hover:text-accent-foreground`；destructive 分支对称追加 `hover:bg-destructive/10 hover:text-destructive`。
  - `DropdownMenuCheckboxItem`、`DropdownMenuRadioItem`、`DropdownMenuSubTrigger`：同法追加 `hover:bg-accent`（SubTrigger 补 `hover:text-accent-foreground`）。
- 不可点项（Label/Separator/滚动按钮）不加。
- 依据：CDP 实测注入 `[role=option]:hover{background:…}` 后 Dialog 内立即生效；`:hover` 不依赖焦点，Dialog focus-trap 不影响。鼠标悬停与键盘 focus 高亮现在双轨并存，互不干扰。
- 无测试缝价值在 jsdom（`:hover` 不渲染），采取轻断言：不新增针对 hover 的渲染测试，靠 code-review + 浏览器/CDP 人工验收。若既有测试对类串快照敏感则同步更新。

### D2 — 分组折叠（复用 chat 模式，三处内联实现）

不抽公共组件/hook（仅 3 个调用点；chat 与 VirtualModelEditDialog 已有内联先例）。每处新增 `collapsed: Set<key>` state（默认空 = 全展开）+ toggle 函数 + 分组标题行改为 `<button>`（`aria-expanded` + ChevronDown/ChevronRight 切换 + 现有 hover 类）。

- **D2.1 `multi-select.tsx`**：`Row` 类型 header 分支渲染为可点按钮。折叠 state 用 `Set<groupLabel>`（group 为字符串）。折叠的组不渲染其 option 行（全选行与搜索框保留）。无 `group` 的选项（rows 里无 header）不显示任何折叠 UI，行为不变。
- **D2.2 `pages/provider-models.tsx`**：搜索联想浮层内每个 `<div data-testid=provider-model-search-group-{id}>` 的组标题 `<p>` 改为按钮；折叠 state `Set<providerId>`；折叠的组不渲染成员按钮，标题按钮保留。
- **D2.3 `pages/virtual-models.tsx`**：同上，state `Set<virtualModelId>`，组标题改按钮。
- **D2.4 `pages/chat.tsx`**：不改（已支持折叠，交互一致默认全展开）。

图标：复用 `lucide-react` 已引入的 ChevronDown/ChevronRight（chat 同款）。

### D3 — 范围限定

- 分组折叠只针对「浮窗/弹层内分组列表」。页面主体卡片分区（如 ProviderModelSection 卡片）不属本需求。
- 折叠状态不持久化（无 localStorage）；组件级 useState，**页面会话内保持**（关闭浮窗/重开搜索不重置，与 chat 页模型选择器行为一致；用户拍板「会话内即可」）。
- 搜索联想浮层折叠后组标题仍可见（点击可再展开），仅隐藏成员。

## Testing Decisions

- **好的测试** = 测外部可见行为：分组标题点击后成员项出现/消失、默认展开、`aria-expanded` 状态、无分组时无折叠控件；不测内部 state 结构。
- **折叠交互组件测试**（jsdom + fireEvent，现有设施）：
  - `multi-select.test.tsx`：分组选项渲染标题按钮；点击标题折叠后其下选项不可见、再点展开恢复；无 group 选项不渲染标题按钮。
  - `__tests__/provider-models-page.test.tsx`、`__tests__/virtual-models-page.test.tsx`：现有搜索分组用例扩展——组标题可点击折叠/展开，折叠后组内成员按钮不可见、标题仍在；默认全展开。
  - `components/__tests__/request-logs.test.tsx`：模型过滤 MultiSelect 分组（复用现有用例数据）——折叠一组后该组模型不在弹层内。
- **hover 类改动**：轻断言（若有既有对类名的断言则补 `hover:bg-accent` 期望；无则不新增渲染测试）。
- 回归：既有测试全绿（`pnpm vitest run`）+ `pnpm lint`。

## Out of Scope

- 不调色板/accent 色值；不动非 Dialog 场景已有的正常 hover 逻辑。
- 不做折叠态持久化、不抽公共可折叠组件/hook、不改 chat 页、不加依赖。
- 不修手写浮层（已全部有 hover）；不处理 Radix Dialog focus-trap 本身（选择在 UI 层规避，`data-highlighted` 键盘可达性保留）。
- 无后端改动、无 E2E。

## Further Notes

- 修复优先级：D1（hover）是明确的可见 bug 修复；D2 是交互增强。
- hover 双轨（`:hover` + `:focus`）在触屏无 hover 场景无影响（触屏无 hover）；键盘用户仍走 focus 高亮。
- 弹窗内 DropdownMenu 场景当前代码里较少（布局/用户菜单在页面），但原语统一加可避免同类问题复发。
