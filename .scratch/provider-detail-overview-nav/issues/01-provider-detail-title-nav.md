# 01: 供应商详情卡标题变「名称 + 方向键」链接跳数据面板

**What to build:** 供应商管理页（`/providers`）右侧详情卡（`ProviderDetail`）的标题——原本是纯文本
供应商名——改为可点击链接：鼠标悬停时标题文字与方向键高亮并轻微右移，点击整页导航到该供应商的
数据面板 `/providers/{id}/overview`。长供应商名仍中间省略截断；baseUrl 副行、启停开关、底部更多菜单
等其它区域不受影响。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] `ProviderDetail` 标题区渲染 react-router `Link`，目标 `/providers/{provider.id}/overview`
- [ ] 标题结构对齐既有导航模式：供应商名包 `MidEllipsis` + ChevronRight 方向图标，
      hover 高亮（`hover:bg-muted/60`）与图标右移仅作用于文字+图标
- [ ] `title` tooltip 复用 i18n key `providerModels.viewProviderOverview`（插值 `{ provider }`），不新增 key
- [ ] 组件测试补断言：渲染后存在 href 指向 `/providers/{id}/overview` 的链接、含供应商名，带数据面板 tooltip
- [ ] `cd web && pnpm vitest run` 全绿、`pnpm lint` 无新增告警
