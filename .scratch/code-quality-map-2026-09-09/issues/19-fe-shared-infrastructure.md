# 19 · FE 共享组件与基础设施域审查

Type: task
Status: claimed
Blocked by: 01

## Question

对前端共享基础设施域做全量审查：`components/ui/`（shadcn 基础组件含 chart/sidebar）、`components/data-table/`（react-table 封装族）、通用组件（mid-ellipsis/confirm-dialog/skip-to-main/theme-toggle/empty-state/multi-select 等）、`lib/`（api.ts ky 封装/utils/constants/pages/backup）、`types/`、`App.tsx`/`main.tsx`/`test/setup.ts`。**范围注记（01 盘点）：hooks 归各消费域（16=stats/race/dashboard、17=CRUD、18=cron/settings、21=auth），本票不审 hooks 本体，只审 lib/api.ts 与 hooks 共享的 stats-query/race-period 等 lib 基础设施。**按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：ky 封装的错误映射与取消、MidEllipsis 自适应测量、data-table 分页/排序/列显隐状态、主题三态与 localStorage polyfill 边界；
- 实现简洁：ui 组件是否被业务代码绕过、共享层重复封装面；
- 测试覆盖：setup.ts 之外缺什么（api 层错误分支、utils 纯函数）；
- 模块间调用：ui 组件与业务组件间的依赖方向（业务是否直用 Radix）、constants 与后端契约同步面。

产出 `.scratch/code-quality-map-2026-09-09/findings/19-fe-shared-infrastructure.md`，Answer 给摘要与需拍板问题。

## Answer

**清单产出**：`findings/19-fe-shared-infrastructure.md`——22 条（4 P2 + 18 P3）无拍板。双子代理分域全读（组件族 / lib+装配），全部 P2 经主代理复核（19-02/19-03 附 ky 1.14.3 node_modules 源码实证）。

**四张 P2**：19-01 sidebar 折叠状态 cookie 只写不读（shadcn SSR 遗产，SPA 无读者，刷新必回展开）；**网络栈三兄弟**——19-02 beforeError 换 ApiError 类型使 ky `isHTTPError` 判不上 → `retry.statusCodes` 白名单失效（GET 4xx 也被重试、Retry-After 被绕过）、19-03 NETWORK_ERROR 分支不可达（网络/超时不过 beforeError，toast 显示英文原始消息）、19-04 全局 10s 超时短于用量上游 15s（usage/estimate/import 未覆盖，前端先失败后端实成功）。

**P3 要点**：MidEllipsis 每实例强制同步重排×46 实例+零宽渲染「…」/multi-select 硬编码 id 一页 4 实例/chart tooltip 吞 0（潜在）/sidebar 快捷键无守卫+移动端首帧闪/desktop 面；/login 懒加载在 Suspense 外直达白屏；双 retry 叠加最多 4 次尝试；401 跳转丢 from 回跳；api.ts 零测试+ui/data-table 零测试目录+utils 五函数裸面；时区键三处定义+PAGES 死码+formatDateTime 硬编码 zh-CN+manualChunks 漏 react-router 核心+setup 全局 mock 掩盖真实 useStatsTimeZone。

**已核验无问题区**：依赖方向单向（业务零直用 Radix）、chart 无 CSS 注入、data-table 分页契约（table-core 8.21.3 实证）、confirm-dialog 嵌套安全、theme 三态+首屏防闪、constants 与后端枚举逐一对齐、NAV_GROUPS 与路由对齐、setup polyfill 完整。

**与 17 票交接**：DialogScrollShell 布局原语定案归本域（ui/ 层）实施批落地。

**需拍板问题**：无。

Status: resolved
