# 02: 下线 5 个页面工具栏刷新按钮

**What to build:** 删除供应商 / API Key / 定时任务 / 供应商模型 / 虚拟模型 这 5 个页面标题栏里各自的刷新按钮，刷新统一由顶栏承担（顺带消除虚拟模型页「刷新只覆盖三个查询中的一个」的覆盖面缺口）。各页错误态的「重试」保留不变；用量卡「刷新用量」、添加弹窗「尝试刷新」、两处整页重载、请求日志「重置」均不动。

同时在这 5 个页面的现有测试里各加一条断言：刷新按钮已不存在，锁定删除不被静默回退。

**Blocked by:** 01（先立新语义，再撤回页面级入口）

**Status:** done (2026-09-10)

- [x] 5 个页面标题栏不再渲染刷新按钮；随之失去引用的 `RefreshCw` import 与局部变量已清理（`refetch`/`refetchProviders`/`refetchModels` 仍被错误态使用，保留），无 lint 警告
- [x] 5 个页面测试各含一条「刷新按钮不存在」（`queryByRole("button", { name: "刷新" })` 为 null）断言
- [x] 各页 ErrorState 重试路径行为不变（既有测试全绿）
- [x] 用量卡、添加模型弹窗、race-card、request-logs、cron-job-logs 的既有测试不改且全绿
- [x] `cd web && pnpm lint && pnpm vitest run` 全绿（65 文件 / 509 测试）
