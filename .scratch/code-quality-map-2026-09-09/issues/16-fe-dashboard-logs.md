# 16 · FE 数据面板与日志域审查

Type: task
Status: open
Blocked by: 01

## Question

对前端数据面板与日志域做全量审查：`pages/overview.tsx` / `api-key-overview.tsx` / `model-overview.tsx` / `request-logs.tsx` + `components/dashboard-charts.tsx` / `insight-charts.tsx` / request-logs 组件族 / `lib/race-period.ts` 及对应 hooks 与 `__tests__`。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：图表数据变换（topWithOther/桶归并/时区换算）与后端契约一致性、race 周期计算、空态/loading/error 分支；
- 实现简洁：图表配置重复、三组三态图表的收敛面（既有收敛不复查）；
- 测试覆盖：页面级 17 件之外缺什么（图表纯函数单测、数据变换边界）；
- 模块间调用：hooks 与 api.ts 的类型契约、与后端 stats 端点字段的同步面。

产出 `.scratch/code-quality-map-2026-09-09/findings/16-fe-dashboard-logs.md`，Answer 给摘要与需拍板问题。
