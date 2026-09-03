# 01: 供应商详情 —— 额外配置 / 自定义请求头可折叠

**What to build:** 供应商详情页中「额外配置」与「自定义请求头」两个 JSON 展示区改为可折叠：标题行（左侧标签 + 右侧方向键）默认折叠，点击标题行整块区域或方向键按钮即可展开/收起，展开内容带轻量淡入动画；展开态在切换供应商时重置回折叠。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 「额外配置」与「自定义请求头」区默认折叠，只显示标题行（标签 + 右侧方向键）；extra/customHeader 为 `"{}"` 或空时不渲染该区。
- [ ] 点击标题行整行区域或方向键按钮可展开/收起；`aria-expanded` 状态正确。
- [ ] 展开内容带 `animate-in` 轻量淡入下滑动画；收起无动画。
- [ ] 展开态在切换供应商（选中另一供应商）时重置回折叠；期间其他操作不自动收起。
- [ ] 组件行为测试（扩展 `provider-detail.test.tsx`）覆盖：默认折叠、点标题行/方向键切换、切供应商重置、空 extra 不渲染；vitest + biome + tsc 全绿。

## Comments

- 2026-09-03 完成。提交 419d1fd（feat: 供应商详情额外配置/自定义请求头改为可折叠）。实现：组件内 `CollapsibleSection`（自持 open state + 整行 button + ChevronRight rotate-90 + animate-in 展开），两区以 `key={provider.id}` remount 实现切供应商重置折叠；测试扩展 provider-detail.test.tsx（新增 6 用例，后按 ponytail-review 精简为 4 个 describe 内断言，最终 9/9 通过）。前端门禁全绿：biome、tsc、vitest 263→261 全过。评审（code-review + ponytail-review）后按建议把受控 state 改回组件自持 + key remount（净减 ~28 行）。
