# REQUIREMENTS — 供应商详情页：额外配置/自定义请求头可折叠

## Scope

`web/src/components/providers/ProviderDetail.tsx`（供应商详情右侧卡片的详情展示）。

将两个原始 JSON 展示区做成**可折叠区**：

1. 「额外配置」(`providers.extraConfig`，键值 `Input` 网格)
2. 「自定义请求头」(`providers.customHeader`，`pre` 块)

## Refined requirements (grilled)

- **覆盖范围**：两个区域都折叠，交互一致（用户拍板）。
- **默认折叠**：初始渲染时两个区均为折叠态。
- **展开态生命周期**：点开后保持展开，直到**切换供应商**时重置回折叠（复用现有明文 API Key 的 `previousId` 重置模式）(用户拍板)。编辑/刷新/开关 enable 等操作不自动收起。
- **标题行**：整行可点横条 —— 左侧沿用现有小号大写标签样式，整行 hover 有底色提示可点（用户拍板）。
- **方向键**：在标题行**右侧对齐**，作为展开状态指示 + 交互元素；折叠→右向箭头，展开→下向（用户原文「折叠方向键和额外配置标题同行，向右对齐」）。点**按钮**或**标题行整块区域**都可展开（用户原文要求「点击按钮或者标题行所在的整个区域展开」）。
- **动画**：展开内容带轻量 fade-in + 轻微向下滑入（`animate-in fade-in slide-in-from-top-2 duration-200`，tailwindcss-animate，nyro 同款）；收拢无动画（用户拍板）。
- 显示条件不变：`provider.extra` 非空且非 `"{}"` 才渲染额外配置区；`provider.customHeader` 非空且非 `"{}"` 才渲染自定义请求头区。

## Non-goals (ponytail cuts)

- **不新增通用 `ui/collapsible.tsx` 原语**：仅 2 处使用，做成本地小组件即可；出现第 3 处使用再抽取。
- **不新增 i18n key**：可访问名由可见标题 + `aria-expanded` 承担。
- **不动后端**。
- **不引入新依赖**：lucide `ChevronRight` 已装，展开时 `rotate-90` 即等价下向箭头。

## Open questions resolved by grilling

- 覆盖范围：两个区都折叠（非仅额外配置）。
- 生命周期：点开保持、切供应商重置（非每次刷新收起）。
- 形态：整行可点横条 + hover 底色（非仅按钮可点）。
- 动画：轻量展开动画（非纯显隐）。
