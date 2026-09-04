# 02: 今日 summary hook + i18n 文案

**What to build:** 前端 `useDashboardSummary` 支持可选 `startTime`/`endTime` 参数（透传
`/api/stats/summary`），query key 纳入参数使累计/今日成为独立缓存条目；新增双语文案 key
「今日」/「Today」（en + zh-CN）。今日窗口边界由页面层计算后传入（本票不涉及页面渲染）。

**Blocked by:** 01（summary 后端参数先落地，前端才能依赖该契约）

**Status:** ready-for-agent

- [ ] hook 传参数正确拼 query，query key 区分累计/今日
- [ ] en/zh-CN 均含今日副标题文案
- [ ] 前端类型检查绿
