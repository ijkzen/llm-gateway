# REQUIREMENTS — 浮窗列表项 hover 修复 + 分组折叠

日期：2026-09-07
来源：用户原始请求（start_work）

> 「你搜索一下现在项目前端有哪些下拉框的浮窗的列表项是没有hover特效的，你要给它加上。然后再有一个，有哪些下拉框或者搜索结果的列表是分组显示的，你需要为这些分组添加折叠和展开功能。」

## 背景与根因（已 CDP 实证）

用户报告「供应商和供应商模型弹窗里的下拉浮窗都没有 hover 背景」，同时「请求日志的结果过滤项的下拉浮窗有 hover」。

排查结论（headless Chrome + CDP 实测，`Input.dispatchMouseEvent` 真实鼠标 + 计算样式采样）：

1. 全站浮窗列表项分两类：
   - **Radix 原生项**（`SelectItem`、`DropdownMenuItem` 等）：hover 高亮依赖「鼠标悬停 → Radix `onPointerMove` → 项获得 focus → `data-highlighted` 置位 → `focus:bg-accent` 生效」。
   - **手写浮层项**（chat 模型选择、MultiSelect、搜索联想、模板联想）：直接用 CSS `:hover`，无此问题（全部已有 hover 背景）。
2. **根因**：Radix Dialog 是 modal，自带 **focus-trap**。Select/Dropdown 的浮层 portal 到 `body`（在 Dialog 焦点域外），用户悬停选项时 Radix 调用 `option.focus()`，焦点立即被 Dialog focus-trap 拉回 trigger，`data-highlighted` 永不置位 → `focus:bg-accent` 永不生效。
   - CDP 对照：同一 `SelectItem` 组件，供应商新建弹窗内 hover「Gemini」`data-highlighted` 恒为 false、背景透明；请求日志页（非 Dialog）hover「失败」`data-highlighted=true`、背景 `rgb(240,242,245)`。
   - 直接对 Dialog 内 option 调 `.focus()`，activeElement 立刻弹回 combobox（`focus failed -> combobox.BUTTON`）。
3. **修复可行性已验证**：向页面注入 `[role=option]:hover { background-color: … }` 纯 CSS，Dialog 内 hover 立即生效（不依赖 focus，focus-trap 拦不住）。

影响面：所有嵌套在 Dialog 内的 Radix Select/Dropdown（供应商新建/编辑、供应商模型详情、虚拟模型编辑的协议/付费/策略/接口类型下拉等）。页面内下拉本就正常，但统一加 `:hover` 后行为一致、无副作用。

## 需求 1 — 浮窗列表项 hover 背景修复

给 `web/src/components/ui/` 原语中所有可点击列表项补 **CSS `:hover` 背景**（保留原 `focus:` 样式不动，键盘可达性不受影响）：

- `select.tsx` → `SelectItem`：`focus:bg-accent` 基础上加 `hover:bg-accent`（及 `hover:text-accent-foreground`）。
- `dropdown-menu.tsx` → `DropdownMenuItem`、`DropdownMenuCheckboxItem`、`DropdownMenuRadioItem`、`DropdownMenuSubTrigger` 同法。
- 不可点项（`SelectLabel`、`SelectSeparator`、`DropdownMenuLabel`、`DropdownMenuSeparator`、滚动按钮）不加。
- 破坏性项（`DropdownMenuItem variant="destructive"`）：`hover:bg-destructive/10 hover:text-destructive` 与 `focus` 对称。

无需动手写浮层（已全部有 hover）；无需调色板。

## 需求 2 — 分组列表折叠/展开

对「分组显示的浮窗列表」加分组折叠，默认**全展开**，折叠状态**组件内 useState（会话内）**，不持久化。分组标题行变为可点击按钮：左侧 Chevron（展开=ChevronDown / 折叠=ChevronRight）+ 标题 + 组内数量（可省）。样式与 chat 页现有模式一致（`aria-expanded` + `hover:bg-accent`）。

四处（用户已确认「四个分组列表全加」）：

1. **`multi-select.tsx`**（request-logs 模型过滤按供应商分组）：分组标题 `<p>` 改为可点折叠按钮；有 `group` 的选项列表才显示折叠。无 group 的选项（provider/vm 过滤）不显示折叠按钮。
2. **`chat.tsx` 模型选择器**：已有折叠（`collapsed: ReadonlySet<number>` + `toggleGroup`），**无需改动**。
3. **`provider-models.tsx` 搜索联想浮层**：按供应商分组，新增折叠（`collapsed: Set<number>` 按 providerId）。
4. **`virtual-models.tsx` 搜索联想浮层**：按虚拟模型分组，新增折叠（`collapsed: Set<number>` 按 virtualModelId）。

搜索联想浮层折叠行为：折叠的组不渲染成员（点开即选中的结果项隐藏）；分组标题按钮点击切换。

## 非目标（ponytail 裁剪）

- 不做 localStorage 持久化折叠态（会话内即可）。
- 不抽公共「可折叠分组」组件/hook（三处各写内联 ~10 行，为 3 个调用点建抽象是过度设计）。
- 不改 chat 页（已支持）。
- 不引入任何新依赖；图标复用 lucide 已有 `ChevronDown`/`ChevronRight`。
- 不动后端、不动手写浮层已有 hover、不改色板。
- 搜索联想浮层的「空分组」本就因无结果而不渲染，无需额外折叠处理。

## 验收

- 弹窗内下拉（供应商/供应商模型/虚拟模型编辑）hover 各项有可见背景。
- 页面内下拉（请求日志等）hover 行为不变（仍可见）。
- request-logs 模型过滤 MultiSelect 分组可折叠/展开，默认展开。
- provider-models / virtual-models 搜索联想分组可折叠/展开，默认展开。
- 前端测试（新增折叠交互用例）与既有测试全绿；`pnpm lint`、`cargo` 侧不受影响。
