# 02: MultiSelect 分组折叠/展开

**What to build:** 请求日志页的「上游模型」多选过滤下拉按供应商分组，现分组标题为静态文字。改为：分组标题行可点击（Chevron 指示 + `aria-expanded`），点击折叠/展开该组；默认全展开；折叠的组其下选项不渲染（「全选」行与搜索框不受影响）；无 `group` 的选项列表（如供应商、虚拟模型过滤）不出现折叠控件、行为不变。折叠态组件内 useState（会话内，不持久化）。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 带 group 的选项：分组标题为可点按钮，点击折叠后组内选项消失、再点恢复；默认全展开
- [ ] 折叠/展开时 `aria-expanded` 与 Chevron 方向正确
- [ ] 无 group 的选项列表不渲染分组标题/折叠控件
- [ ] 折叠后搜索、勾选等既有行为不回归
- [ ] multi-select.test.tsx 与 request-logs.test.tsx 折叠用例 + 全量 `pnpm vitest run` 全绿
