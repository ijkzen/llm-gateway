# 19 · FE 共享组件与基础设施域审查

Type: task
Status: open
Blocked by: 01

## Question

对前端共享基础设施域做全量审查：`components/ui/`（shadcn 基础组件含 chart/sidebar）、`components/data-table/`（react-table 封装族）、通用组件（mid-ellipsis/confirm-dialog/skip-to-main/theme-toggle/empty-state/multi-select 等）、`lib/`（api.ts ky 封装/utils/constants/pages/backup）、`types/`、`App.tsx`/`main.tsx`/`test/setup.ts`。**范围注记（01 盘点）：hooks 归各消费域（16=stats/race/dashboard、17=CRUD、18=cron/settings、21=auth），本票不审 hooks 本体，只审 lib/api.ts 与 hooks 共享的 stats-query/race-period 等 lib 基础设施。**按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：ky 封装的错误映射与取消、MidEllipsis 自适应测量、data-table 分页/排序/列显隐状态、主题三态与 localStorage polyfill 边界；
- 实现简洁：ui 组件是否被业务代码绕过、共享层重复封装面；
- 测试覆盖：setup.ts 之外缺什么（api 层错误分支、utils 纯函数）；
- 模块间调用：ui 组件与业务组件间的依赖方向（业务是否直用 Radix）、constants 与后端契约同步面。

产出 `.scratch/code-quality-map-2026-09-09/findings/19-fe-shared-infrastructure.md`，Answer 给摘要与需拍板问题。
