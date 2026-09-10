# 16 · FE 数据面板与日志域审查

Type: task
Status: claimed
Blocked by: 01

## Question

对前端数据面板与日志域做全量审查：`pages/overview.tsx` / `api-key-overview.tsx` / `model-overview.tsx` / `request-logs.tsx` + `components/dashboard-charts.tsx` / `insight-charts.tsx` / request-logs 组件族 / `lib/race-period.ts` 及对应 hooks 与 `__tests__`。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：图表数据变换（topWithOther/桶归并/时区换算）与后端契约一致性、race 周期计算、空态/loading/error 分支；
- 实现简洁：图表配置重复、三组三态图表的收敛面（既有收敛不复查）；
- 测试覆盖：页面级 17 件之外缺什么（图表纯函数单测、数据变换边界）；
- 模块间调用：hooks 与 api.ts 的类型契约、与后端 stats 端点字段的同步面。

范围注记（01 盘点）：本票域内含 stats/race/dashboard hooks（use-dashboard-*/use-stats-*/use-*-race/use-model-metrics 等）；hooks 为跨域共享接口（RequestLogsTable 等 17 域组件亦消费），视为冻结。

产出 `.scratch/code-quality-map-2026-09-09/findings/16-fe-dashboard-logs.md`，Answer 给摘要与需拍板问题。

## Answer

**清单产出**：`findings/16-fe-dashboard-logs.md`——17 条（7 P2 + 10 P3）无拍板。双子代理分域全读（概览页+赛马族 / 图表组件+请求日志族），全部 P2 与关键 P3 经主代理行号磁盘复核。

**七张 P2**：16-01 `?period=foo` 非法 URL 参数穿过无 default 的 switch 链 → `formatToParts(NaN)` RangeError → 整页 ErrorBoundary（所有含赛马卡页面可构造）；16-02 virtual-model/provider 两个概览页缺 detail 404/非法 id 错误态（api-key/model 两页有守卫，NaN id 还触发后端 400 散块）；16-03 请求日志时间窗口变更不重置 page（其余 7 个变更点都重置）→ 越界页空态假死；16-04 请求日志 4 列头可排序但不在后端白名单 → 静默回退 start_time 箭头说谎；16-05 insight 五图图例硬编码中文（EN 界面图例中文、同图 tooltip 已英文化）；16-06 X 轴桶标签用浏览器本地时区（设置表时区≠浏览器时区时整体错位，默认同上海不可见）；16-07 RPM/TPM 同 Y 轴量级失配 RPM 贴底不可读。

**P3 要点**：16-08 自定义窗口全链路浏览器本地时区（与预设口径不齐）/16-09 ApiKeyRaceCard 缺 initialWindow（四卡复制漂移实例）/**16-10/16-11=01 盘点两个骨架同构候选的定案：四赛马卡抽 MetricRaceCard、三二级页抽 AnalysisSections 区块层（整页装配不抽=配置爆炸）均不触碰 hooks 冻结接口**/16-12 dashboard-charts 多余 export+labelInterval 重复（**insight 语义色定案不统一**）/16-13 CSV 生成侧无转义（key 名含逗号错配，呼应 11-16）/16-14 inferGranularity 无 year 未爆弹/16-15 测试注释漂移/16-16 赛马表全量渲染（前后端协同项）/16-17 测试缺口族（initialWindowFromUrl 真实函数未被测——往返测试是复刻版，16-01/16-03/16-04 均无回归网）。

**已核验无问题区**：rank 五端点契约逐字段对齐、删除主体行守卫、预设窗口 tz 内核（上海/纽约 DST 断言）、insight 位置索引安全、无轮询+桶数有界。

**需拍板问题**：无。

Status: resolved
