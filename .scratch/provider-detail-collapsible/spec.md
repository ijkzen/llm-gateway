# Spec: 供应商详情 —— 额外配置 / 自定义请求头可折叠

Status: ready-for-agent

## Problem Statement

供应商详情页（右侧卡片）中，「额外配置」与「自定义请求头」两个原始 JSON 展示区始终完整展开。对 extra 字段多（含 `refresh_token`、`ak/sk` 等敏感凭据的掩码占位）或 customHeader 很长的供应商，详情页纵向被拉得很长，基本字段（API Key、协议、计费、代理、时间）需要滚动才看得到全貌。

## Solution

把这两个区做成可折叠区：标题行（左侧标签 + 右侧方向键）默认折叠，点标题行整块区域或方向键可展开/收起，展开带轻量动画；点开后保持展开，直到切换供应商时重置回折叠。

## User Stories

1. 作为管理员，我打开供应商详情时，「额外配置」区默认折叠、只显示标题行，以便页面纵向更紧凑，基本字段无需滚动即可看全。
2. 作为管理员，我打开供应商详情时，「自定义请求头」区默认折叠、只显示标题行，以便同样的紧凑效果。
3. 作为管理员，标题行右侧有一个方向键指示当前状态（折叠→右向箭头，展开→下向箭头），与标题同行、向右对齐，以便我一眼看出该区当前可否展开。
4. 作为管理员，我点击方向键按钮可以展开/收起该区，以便按需查看原始 JSON。
5. 作为管理员，我点击标题行所在的整块区域也能展开/收起（不必精确点到按钮），以便大点击目标操作。
6. 作为管理员，我展开某区后，展开态一直保持（期间编辑供应商、开关 enable、刷新用量等操作不会把它悄悄收起），以便我连续查看。
7. 作为管理员，我切换到另一供应商后，该区的展开态被重置回默认折叠，以便新供应商从紧凑状态看起、且不与旧供应商的浏览状态混淆。
8. 作为管理员，展开内容区以轻量淡入 + 轻微下滑动画呈现，以便状态切换有明确视觉反馈。
9. 作为管理员，extra/customHeader 为空或 `"{}"` 的供应商不显示对应区（含标题行），以便不展示无意义的空折叠区。

## Implementation Decisions

### 改动范围

- 仅改 `ProviderDetail` 组件（供应商详情卡片）。后端零改动，无新依赖。
- 两个区复用同一交互与视觉，抽成一个**组件文件内**的小型内部组件（如 `CollapsibleSection`），不新增 `ui/` 下的通用原语——当前仅两处使用，出现第三处再上提。

### 组件形态

- `CollapsibleSection` 接收：标题文案、默认折叠、children（内容区）。对外呈现：
  - **标题行** = 整行可点（`<button>`，`w-full`），左侧沿用现有小号大写标签样式（`text-xs font-medium uppercase tracking-wider text-muted-foreground`），右侧 `ChevronRight` 图标；展开时图标 `rotate-90` 变为下向。行内 `justify-between` 实现「标题与方向键同行、方向键右对齐」。
  - **hover 态**：标题行 hover 有底色提示可点（圆角浅背景），与 nyro 参考一致。
  - **内容区**：展开时渲染，带 `animate-in fade-in slide-in-from-top-2 duration-200`（tailwindcss-animate 已装，nyro 同款）。收起不渲染、无动画。
  - 可访问性：标题行 `<button aria-expanded>`；方向键为纯指示，按钮的可用名由可见标题文本承担（`sr-only`/图标不额外加文案，避免新增 i18n key）。

### 状态生命周期

- 在 `ProviderDetail` 内维护展开态（两个布尔 state，如 `extraOpen`/`customHeaderOpen`），初值均折叠。
- 切换供应商时重置为折叠：复用现有明文 API Key 的 `previousId` 重置模式（`useRef` 比较 `provider.id`，变化即重置本地态），在现有 `setPlainKey(null)` 处一并 `setExtraOpen(false)`/`setCustomHeaderOpen(false)`。

### 显示条件

- 现条件不变：`provider.extra && provider.extra !== "{}"` 才渲染额外配置折叠区；`provider.customHeader && provider.customHeader !== "{}"` 才渲染自定义请求头折叠区。`ProviderUsageCard`（用量信息）渲染逻辑与位置不变。

## Testing Decisions

- **接缝**：组件行为测试，扩展现有 `web/src/components/__tests__/provider-detail.test.tsx`（已有 ProviderDetail 渲染与 mock 基建，import Provider/Toast mocks）。测试只断言外部可见行为（内容是否渲染、`aria-expanded` 值、点击后显隐），不断言内部 state 或 DOM 结构细节。
- **覆盖用例**：
  1. 默认折叠：extra/customHeader 非空时，标题行可见但内容键值/pre 块不可见；`aria-expanded` 为 false。
  2. 点标题行整行展开：内容出现；再点标题行收起。
  3. 点方向键按钮展开/收起。
  4. 展开态在切换供应商（rerender 新 provider）后重置为折叠。
  5. extra/customHeader 为 `"{}"` 时不渲染标题行。
- **先前范式**：现有 `provider-detail.test.tsx` 用 `fireEvent` + `getByRole` 断言交互；`provider-usage-card.test.tsx`、`cron-jobs` 相关测试同为组件行为测试范式。

## Out of Scope

- 不新增通用 `ui/collapsible.tsx` 原语。
- 不新增 i18n key（不显示「展开/折叠」文字）。
- 不改后端与 API。
- 不改 `ProviderUsageCard` 及用量信息展示。
- 不做收拢动画（仅展开动画）。

## Further Notes

- 视觉基准 nyro（`webui/src/components/resource-form.tsx` 的「高级设置」区）同款交互：整条可点横条 + 右侧方向键，方向键用图标旋转表达状态；本实现以 `ChevronRight` + `rotate-90` 达成，不引入第二个图标。
- 展开动画复用项目现有 `animate-in` 系列工具类，不引入新依赖。
