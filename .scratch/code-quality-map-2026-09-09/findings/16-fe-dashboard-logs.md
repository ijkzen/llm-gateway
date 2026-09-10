# FINDINGS · 16 FE 数据面板与日志域审查（2026-09-10）

范围：四概览页（overview/api-key-overview/model-overview/virtual-model-overview + provider-overview）+ 赛马族（四卡/shell/window-control/race-period.ts 435 行）+ 图表组件（dashboard-charts 363/insight-charts 531/insight-analysis-card）+ 请求日志族（request-logs 页壳 + components/request-logs）+ 对应 hooks 与全部相关 `__tests__` 盘点。方法：两个子代理分域全读 + 主代理对全部 P2 与关键 P3 行号磁盘复核。清单模式：不改代码。hooks 冻结接口约定全程未触发变更建议。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 16-01 | P2【已修复 2026-09-10】 | 逻辑/健壮 | `initialWindowFromUrl` 不校验 URL `period`：非法值穿过无 default 的 switch 链 → `new Date(NaN)` → `formatToParts` RangeError → 整页 ErrorBoundary（任何含赛马卡的页面，`?period=foo` 可构造） |
| 16-02 | P2【已修复 2026-09-10】 | 逻辑/分支 | virtual-model-overview 与 provider-overview 缺 detail 404/非法 id 错误态（api-key/model 两页有守卫）：已删 VM 继续发 stats 查询，NaN id 触发 400 散块而非明确错误页 |
| 16-03 | P2【已修复 2026-09-10】 | 逻辑/联动 | 请求日志时间窗口变更不重置 `page`（其余 7 个变更点都重置）→ 收窄窗口后停在越界页显示「暂无请求日志」假死，只能逐页退回 |
| 16-04 | P2【已修复 2026-09-10】 | 模块间/契约 | 请求日志 4 个列头（虚拟模型/供应商/输入/输出）可点排序但不在后端白名单，后端静默回退 start_time——表头箭头与实际排序不符 |
| 16-05 | P2【已修复 2026-09-10】 | i18n | insight 五图图例 `config.label` 硬编码中文（成功/失败/输入/输出/缓存命中率/流式占比），EN 界面图例中文而同图 tooltip 已英文化——同组件语言不一 |
| 16-06 | P2【已修复 2026-09-10】 | 逻辑/时区 | `formatBucketLabel` 用浏览器本地时区换算 X 轴标签；设置表时区 ≠ 浏览器时区时标签整体错位（默认同为上海时不可见） |
| 16-07 | P2【已修复 2026-09-10】 | 可视化 | ThroughputChart 的 RPM（个位量级）与 TPM（万级）共用 `yAxisId="count"`——RPM 线贴底不可读，双折线名不副实 |
| 16-08 | P3【已修复 2026-09-10】 | 逻辑/时区 | 自定义窗口的默认值/输入/展示全走浏览器本地时区，与预设周期的设置表口径不齐（与 16-06 同族不同面） |
| 16-09 | P3【已修复 2026-09-10】 | 逻辑 | `ApiKeyRaceCard` 不接 `initialWindow`（另三卡都接）→ model/virtual-model 概览页深链 `?period=week` 后该卡仍是「天」——四卡复制粘贴漂移实例 |
| 16-10 | P3【已修复 2026-09-10】 | 简洁 | 四张赛马卡逐字同构（各 ~65 行）可抽泛型 `MetricRaceCard`；race-card-shell:87-100 重复了 stats-section 的 `useSectionSubtitle`；不触碰 hooks 冻结接口 |
| 16-11 | P3【已修复 2026-09-10】 | 简洁 | 三二级页「调用/Token/可靠性」装配重复 ~60 行/页可抽 `AnalysisSections`（overview 统一门控与 model 折线变体保留）——01 盘点候选的定案：抽区块层、不抽整页装配 |
| 16-12 | P3【已修复 2026-09-10】 | 简洁 | dashboard-charts 多余 export 面（CHART_COLORS/OTHER_LABEL/modelLabel/chartColorAt 仅内部使用）；labelInterval 两文件重复；OTHER_LABEL 应走 i18n；**insight 语义色不统一为定案**（按 index 轮转反降可读性） |
| 16-13 | P3【已修复 2026-09-10】 | 契约/健壮 | CSV 多值生成侧无转义：不同供应商同名模型产生重复段（无害）；API Key 名含逗号会被后端 split 错配（呼应 11-16 后端静默吞） |
| 16-14 | P3【已修复 2026-09-10】 | 健壮 | `inferGranularity` 无 year 分支（>24h 全判 month）；当前调用方全显式传 granularity 使 fallback 实不执行，属未爆弹 |
| 16-15 | P3【已修复 2026-09-10】 | 测试观察 | race-period 测试注释漂移（描述 36W 断言 35W）；ISO 周数年界（12/29-1/3 归属相邻 ISO 年）无覆盖 |
| 16-16 | P3【保持现状（用户拍板 2026-09-10）】 | 性能 | 赛马表全量行渲染无上限/无虚拟滚动（provider-model-rank 大部署可达数百行）；后端 rank 无分页参数，改动需前后端协同 |
| 16-17 | P3【已修复 2026-09-10（最有价值四项）】 | 测试覆盖 | 缺口族：race-window-control 无专测 / initialWindowFromUrl 真实函数（往返测试是复刻版）/ confirmCustom 非法区间 / 图表纯函数（toRankedModels/inferGranularity）/ buildQuery 序列化零单测 / 列头排序传参（16-04 无回归网）/ 越界分页（16-03）/ 6 个本域 hooks 无直测 |

**本票无需拍板项**（七张 P2 均为明确缺陷、修复方向无取舍；16-12 的「语义色不统一」为分析定案非用户决策）。

## 各条证据

### 16-01 period URL 参数未校验整页崩溃（P2）

race-window-control.tsx:45 `const period = (searchParams.get("period") as RacePeriod | "custom" | null) ?? "day"` 无白名单；下游 switch 链全无 default：periodStart（race-period.ts:71-82）/periodStartInTz（:178-192）/chartGranularity（:318-347）/formatPeriodLabel（:250-304）/formatCompactPeriodLabel（:378-410），未知 period 产 undefined → tz 路径 wallParts(tz, NaN)（:130-131 `formatToParts(new Date(ms))`）V8 抛 RangeError → 渲染期异常被 App 级 ErrorBoundary 捕获 → 整页「出错了」；非 tz 路径则窗口静默丢失退回后端默认。触发面=所有读 URL 的页面（四概览+赛马卡所在页）。默认解：解析处白名单（`["day","week","month","year","custom"].includes(raw) ? raw : "day"`）。回归测试归 16-17。

### 16-02 两个概览页缺 detail 错误态（P2）

对照组：api-key-overview.tsx:92 与 model-overview.tsx:95 有 `if (!idValid || detailQuery.isError)` 错误态。缺口组：virtual-model-overview.tsx:106-108 只看 `detail.data` 不看 isError、无 idValid 分支，且 useDashboardCharts/useDashboardInsight 无 enabled 门控（:126-143），已删除 VM id 继续发 stats 请求；非法 id（NaN）时后端 `Option<i32>` 反序列化失败 → 400 散块。provider-overview.tsx:81-83 同型缺口（只看 data）。默认解：两页对齐补守卫分支；纯页面层不动 hooks。

### 16-03 时间窗口变更不重置分页（P2）

RequestLogsTable.tsx:489-493 的 RaceWindowControl onChange 只 setTimeWindow；其余变更点（vm:414/provider:428/model:441/status:453/apiKey:476/排序:392/每页条数:570）都 setPage(1)。窗口从「年」切到「天」时 page=N 越界 → items 空、total>0 → EmptyState 假死。resetAll 内已含 setPage(1) 证明属遗漏。默认解：onChange 补 setPage(1)。附带观察：use-request-logs.ts:87 placeholderData 使筛选期 isLoading 恒 false、骨架不再出现，与越界空态叠加更易误判。

### 16-04 列头排序与后端白名单不对齐（P2）

RequestLogsTable 九列 accessorKey（:260-372）：virtualModelDisplayId/providerName/apiKeyName/modelId/success/inputTokens/outputTokens/requestTime/startTime，全部可排序（DataTableColumnHeader 按 getCanSort）。后端 SORTABLE_COLUMNS（request_logs.rs:28-38）只认 startTime/requestTime/totalTokens/success/apiKeyName/virtualModelId/modelId/ttft/tps——`virtualModelDisplayId`/`providerName`/`inputTokens`/`outputTokens` 四列不在内，未知 sortBy 静默回退 start_time（:193-197 不报错）。用户点「输入」升序 → 箭头亮但数据仍按时间排。默认解：四列 `enableSorting: false`（另注意白名单的 virtualModelId 与前端列名 virtualModelDisplayId 也不对齐）。

### 16-05 insight 图例硬编码中文（P2，i18n）

insight-charts.tsx:176-178（成功/失败/失败率）、:280-282（输入/输出/缓存命中率）、:395（流式占比）的 ChartContainer config.label 硬编码中文；ChartLegendContent 渲染 itemConfig.label（ui/chart.tsx:310）→ EN 界面图例中文；同图 tooltip 已走 i18n（:207-219/:318-324/:429-435），en.ts 已有对应英文键——漏接翻译。默认解：config 由 t() 生成（组件内 useTranslation）。

### 16-06 X 轴桶标签时区错位（P2，时区）

dashboard-charts.tsx:71-93 `formatBucketLabel` 全走浏览器本地（getHours/getMonth/getFullYear）；而后端桶边界按设置表时区对齐。浏览器时区≠设置时区时标签整体偏移（如设置纽约、浏览器上海：纽约 0 点桶显示 12:00）。insight-charts.tsx:44 复用同函数同病。缓解：默认部署与浏览器同上海时不可见。默认解：函数加 IANA timeZone 参数（复用 race-period 的 wallParts/Intl 路径），页面透传 useStatsTimeZone()。

### 16-07 RPM/TPM 同轴量级失配（P2，可视化）

insight-charts.tsx:408-465：rpm（桶调用÷窗口小时，个位量级）与 tpm（桶 token÷60，万级）同 `yAxisId="count"` 线性轴 → RPM 贴底。默认解：tpm 单独 yAxisId（FailureTrendChart/TokenStructureChart 已有双轴先例）。

### 16-08 自定义窗口时区口径（P3）【已修复 2026-09-10】

race-period.ts:427-434 defaultCustomWindow、:307-311 toLocalInputValue、:416-424 formatDateTimeLabel 全为浏览器本地；预设周期走 periodBounds(tz) 设置表口径。浏览器≠设置时区时自定义窗口边界与预设整体偏移、与后端分桶不对齐。默认解：defaultCustomWindow 加 timeZone 参数按设置表时区取整；datetime-local 输入保留本地（原生限制）但注释标注。

### 16-09 ApiKeyRaceCard 缺 initialWindow（P3）【已修复 2026-09-10】

ApiKeyRaceCard.tsx:20-28 props 仅 filter、内部固定 useRaceCardWindow()；另三卡均有 initialWindow。model-overview.tsx:202-203 与 virtual-model-overview.tsx:220 调用不传 → 深链带 period 时同页窗口不一致。默认解：补 prop 对齐签名+两处透传；16-10 抽泛型后自然消。

### 16-10 四赛马卡同构（P3，简洁）【已修复 2026-09-10】

四卡各 ~65 行逐字同构（window→sort→hook→shell→table），差异仅配置项。默认解：抽 `MetricRaceCard`（useRank 以函数值传入合法），四卡退化为 5-10 行配置；race-card-shell.tsx:87-100 副标题逻辑重复 stats-section.tsx:40-47 的 useSectionSubtitle，一并复用。波及面：4 组件+4 测试（可改表驱动），props 不变，不动 hooks 冻结接口。

### 16-11 三区块装配重复（P3，简洁）【已修复 2026-09-10】

api-key-overview.tsx:132-195 / virtual-model-overview.tsx:160-217 / provider-overview.tsx:146-203 逐字重复「调用/Token/可靠性」装配 ~60 行/页。定案（01 盘点候选）：抽 `AnalysisSections` 区块组件（三个 query 结果+窗口+粒度作 props）；overview.tsx 的页面级统一门控与 model-overview 的折线变体保留；每页 hooks 接线不抽（门控异构，硬抽=配置爆炸）。纯页面层不动 hooks。

### 16-12 导出收敛定案（P3，简洁）【已修复 2026-09-10】

dashboard-charts.tsx:28-50 的 CHART_COLORS/OTHER_LABEL/modelLabel/chartColorAt 均仅文件内使用（外部只 import formatBucketLabel/inferGranularity）——多余 export 面。insight 五图用语义色（失败=chart-5 等）非 index 轮转，**定案不统一**（强转反降可读性）。可收敛：labelInterval 两文件重复抽共享 helper；OTHER_LABEL 的 modelId 写死「其他」应走 i18n（label 已是翻译值）。insight 五图 CartesianGrid/XAxis/YAxis 样板重复属可选进一步收敛。

### 16-13 CSV 生成侧无转义（P3，契约）【已修复 2026-09-10】

use-request-logs.ts:64-68 `join(",")` 直拼：①不同供应商同名模型（如两个 gpt-4o）同选产生重复段（无害冗余）；②API Key 名允许含逗号（api_keys.rs:106-108 只校验非空），`apiKey=a%2Cb` 被后端 split 成两段——过滤静默扩大错配（与 11-16 后端侧同族）。默认解：后端改收重复参数或前端拒含分隔符值；前端注释标注约束。

### 16-14 inferGranularity 无 year（P3，健壮）【已修复 2026-09-10】

dashboard-charts.tsx:96-105 >24h 全判 month；chartGranularity 对 >366 天给 year（race-period.ts:346）。当前调用方全显式传 granularity（九处列举核实），`??` 短路使 fallback 不执行——未爆弹。默认解：补 year 分支或删 fallback 强制显式传参。

### 16-15 race-period 测试观察（P3）【已修复 2026-09-10】

race-period.test.ts:186 注释「2026-36W」断言 :188 实为「2026-35W」——注释漂移；isoWeekNumber（:349-361）简化公式的 ISO 年界（12/29-1/3 归属）无覆盖。仅影响周标签文案。

### 16-16 赛马表全量渲染（P3，性能）【保持现状 2026-09-10】

sortable-metric-table.tsx:106-179 无上限全量 map；ProviderModelRaceCard 注释自述 6×50=300 行场景，大部署可上千。默认解：Top-N 截断或 max-height+虚拟滚动；后端 rank 无分页参数，前后端协同项，登记图后。

### 16-17 测试缺口族（P3）【已修复 2026-09-10（最有价值四项）】

已有：race-period 280 行（含上海/纽约 DST）、sortable-metric-table、race-card-shell 五分支、四赛马卡各 ~150 行、四概览页（api-key/model 含 404 守卫）、stats-query、dashboard-utils（topWithOther 11 行合并）、request-logs 17 例、insight-analysis-card 2 例、dashboard-charts 5 例（仅 formatBucketLabel）。缺口：race-window-control 无专测文件；initialWindowFromUrl 真实函数未测（sortable-metric-table.test.ts:79-94 的往返测试是复刻实现非调用真身——16-01 无回归网）；confirmCustom 非法区间静默关闭；图表纯函数（toRankedModels/inferGranularity/chartColorAt/modelLabel）；buildQuery 序列化零单测（CSV/undefined 跳过/编码/16-04 排序传参/16-03 越界分页）；insight 三 Tab 空态与粒度透传；本域 6 hooks 无直测（hooks/__tests__ 仅 use-in-view）；virtual-model/provider 概览页 detail 404/非法 id（16-02）。

## 已核验无问题区（避免后续票重复审查）

- **rank 契约逐字段对齐**：五个 rank 端点前端手写类型与后端 rank_impl 五结构体（camelCase+flatten）逐字段一致无漂移；startTime/endTime/items 信封一致。
- **排序语义**：前端默认 totalTokens desc/耗时类 asc 与后端 sort_direction 一致；点击翻转、后端排序、前端不二次排序。
- **删除主体守卫**：api-key/provider-model/member 三表对 null 主键禁跳（title/tabindex/cursor 清空，测试在）。
- **懒加载与分支序**：useInView once+disconnect；四卡 status 分支序完整（未进视口→loading→error→empty→内容）。
- **预设窗口 tz 内核**：Intl 墙钟+固定偏移+DST 校正，上海/纽约断言在（race-period.test.ts:237-280），与后端设置表口径一致。
- **insight 三图位置索引安全**：后端 call_trend/failure_trend/failure_rate_trend 同组补零桶等长对齐，success=max(0,call-failure) 成立。
- **详情弹窗空值守卫**：tps/cache 字段非空断言与后端一致，可空字段有 — 兜底。
- **buildQuery**：undefined 跳过、URLSearchParams 编码、page/pageSize 恒发、默认排序与后端一致。
- **localStorage 持久化**：try/catch+类型校验。
- **无轮询**：面板与日志均无 refetchInterval，staleTime 5min；桶粒度收敛（hour/day/month/year）数据量有界；labelInterval 自适应。

## 性能/内存轮结论

无 P1/P2 性能项。查询无轮询、staleTime 5min、keepPreviousData 防切窗闪骨架；桶数有界（≤31 点）动画开销可忽略；请求日志分页上限 100 与后端 MAX_PAGE_SIZE 一致+placeholderData；唯一动作项=16-16 赛马表全量渲染（大部署）。modelOptions 构建的 O(供应商×模型) 被 useMemo 缓存，非常数热路径。结论：本域性能形态健康。

## 实施进度（2026-09-10）

- **16-08 已修复**：`defaultCustomWindow` 增加 `timeZone` 参数（按设置表时区取整日边界），`RaceWindowControl` 透传 `useStatsTimeZone()`；`toLocalInputValue` 保留浏览器本地（`datetime-local` 原生限制）并注释标注口径。
- **16-09 已修复**：`ApiKeyRaceCard` 补 `initialWindow` prop 与另三卡签名对齐，model/virtual-model/provider/api-key 四个概览页均透传 URL 初始窗。
- **16-10 已修复**：新增 `components/metric-race-card.tsx`（泛型 `MetricRaceCard<T>` + `raceHref`），四张赛马卡各从 ~65 行退化为 ~40 行纯配置（props 与 hooks 接口均未变）；`useSectionSubtitle` 归位到 `race-window-control.tsx` 供卡片壳与区块壳共用（`stats-section` 保留出口，六个页面调用点零改动）。
- **16-11 已修复**：`stats-section.tsx` 新增 `AnalysisSections`（三区块装配，窗口/粒度/三个查询结果作 props），api-key/virtual-model/provider 三个二级页各删 ~60 行装配；overview 的页面级统一门控与 model-overview 的折线变体按定案保留。
- **16-12 已修复**：`CHART_COLORS`/`OTHER_LABEL`/`chartColorAt`/`modelLabel` 收回文件内可见性；`labelInterval` 抽为 dashboard-charts 导出、insight-charts 改用同一份；`OTHER_LABEL` 注释明确为聚合行内部哨兵（可见文案走 `otherLabel(t)`）。insight 语义色按定案不统一。
- **16-13 已修复**：`csvValue` 过滤含逗号的多选值（后端按逗号 split 会把一个 Key 名错配成两段）。
- **16-14 已修复**：`inferGranularity` 补 year 分支（>90 天按年桶，与 `chartGranularity` 的 >366d→year 对齐）。
- **16-15 已修复**：`race-period.test.ts` 周标签注释漂移纠正（35W），新增「ISO 周界」describe 把简化公式的实际归属行为钉死。
- **16-16 保持现状（用户拍板）**：已加 `max-h-[480px]` 纵向滚动容器解决「整页被撑长」这一可见症状；上千行 DOM 渲染开销可接受，Top-N 截断会隐藏数据、虚拟滚动属前后端协同改造，均不做。
- **16-17 已修复（最有价值四项）**：新增 `initialWindowFromUrl` 直调真实函数 + 非法 period 回归、请求日志「时间窗口变更重置分页（16-03）」与「列头排序传参与四列禁排序（16-04）」回归、`toRankedModels`/`inferGranularity`/`labelInterval` 纯函数测试。其余缺口（race-window-control 专测、confirmCustom 非法区间、buildQuery 序列化、6 个 hooks 直测）按拍板不做。

- **16-01 已修复**：`initialWindowFromUrl` 增加 period 白名单（day/week/month/year/custom），非法值回落 day——`?period=foo` 不再穿透 switch 链产生 `new Date(NaN)` 整页 ErrorBoundary。
- **16-02 已修复**：`virtual-model-overview` 与 `provider-overview` 补齐 idValid/detail-isError 错误态（对齐 api-key/model 两页），并给 metrics/charts/insight 加 `enabled` 门控（已删主体不再发 stats 请求）；新增 `dashboardPage.overviewNotFound*`/`backToList` 双语词条。
- **16-03 已修复**：请求日志的时间窗口变更补 `setPage(1)`（其余 7 个变更点原本就重置，属遗漏）。
- **16-04 已修复**：`virtualModelDisplayId`/`providerName`/`inputTokens`/`outputTokens` 四列 `enableSorting: false`（不在后端排序白名单，点了会静默回退 start_time）。
- **16-05 已修复**：insight 五图 `config.label` 改走 `i18n.t("dashboard.*")`（与同图 tooltip 同源）；并修正 20-02 的 `zh-CN` 里 `dashboard.success/failed` 值为中文。
- **16-06 已修复**：`formatBucketLabel` 增加 `timeZone` 参数（Intl formatToParts 取设置表时区墙钟），`TrendLineChart` 与四个 insight 图接 `useStatsTimeZone()`；补跨时区回归（上海 08:00 / 纽约 20:00 前一日）。
- **16-07 已修复**：ThroughputChart 的 TPM 拆到独立 `yAxisId="tpm"`（RPM 个位与 TPM 万级不再同轴，RPM 线不再贴底）。
