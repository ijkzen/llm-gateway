# 01: Radix 浮窗可点项补 CSS hover 背景

**What to build:** 用户在弹窗内（新建/编辑供应商、供应商模型详情、虚拟模型编辑等）的下拉/菜单里悬停选项时能看到高亮背景。修复 Radix Dialog focus-trap 致选项 `data-highlighted` 永不置位、`focus:bg-accent` 永不生效的问题：给 Select 与 Dropdown 原语的所有可点列表项追加 CSS `:hover` 背景类（不依赖焦点），弹窗内外 hover 一致可见。键盘 focus 高亮保留不变。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Select 浮层项 hover 有可见背景（含弹窗内，CDP/浏览器人工验证）
- [ ] Dropdown 可点项（含 destructive 项）hover 有可见背景
- [ ] 不可点项（Label/Separator/滚动按钮）无 hover 类、行为不变
- [ ] 既有 focus 高亮与键盘导航不受影响
- [ ] `pnpm lint` + `pnpm vitest run` 全绿
