# 04: virtual-models 搜索联想分组折叠

**What to build:** 虚拟模型页顶部搜索联想浮层按虚拟模型分组显示结果。现分组标题为静态文字。改为：分组标题行可点击（Chevron 指示 + `aria-expanded`），点击折叠/展开该组；默认全展开；折叠的组不渲染组内成员结果按钮（组标题保留可再展开）；搜索词变化后折叠态在本次会话内保持。折叠态组件内 useState（按 virtualModelId，不持久化）。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 组标题为可点按钮，点击折叠后组内结果项消失、再点恢复；默认全展开
- [ ] 折叠态不影响搜索与点击结果打开详情
- [ ] virtual-models-page.test.tsx 折叠用例 + 全量 `pnpm vitest run` 全绿
