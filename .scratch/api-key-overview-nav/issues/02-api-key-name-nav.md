# 02: API Key 列表 name 列「名称 + 箭头」跳转入口

**What to build:** 在 API Key 列表页，每个 key 的名称列变为「名称 + ChevronRight 方向键」的导航区，
点击即跳转到该 key 的数据面板 `/api-keys/:id/overview`。同一行的启停开关与行尾操作菜单保持原样、
不被误触。视觉与供应商模型 / 虚拟模型页的模型 ID 导航区一致（hover 底色、箭头微移）。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] ApiKeysTable 的 name 单元格渲染「名称 + ChevronRight」，点击调用编程导航到 `/api-keys/{id}/overview`
- [ ] 点击名称/箭头区域才跳转；点同一行其它区域（启停 Switch、操作菜单）不触发导航
- [ ] 前端测试：mock `useNavigate`，断言点名称触发正确路径、Switch 启停仍工作
