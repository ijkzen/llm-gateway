# 03: API Key 数据面板页骨架 + 顶部 6 指标卡 + 三个趋势/分析区块

**What to build:** 新增独立 API Key 数据面板二级页，路由 `/api-keys/:id/overview`（`:id` 为 key 数字主键）。
页面形态对齐供应商/虚拟模型详情页：顶部标题（key name）+ 若干独立时间窗区块。key 不存在时显示错误态并引导返回。

**Blocked by:** 01（后端 stats 加 apiKey 过滤 + api-key-metrics 端点）

**Status:** ready-for-agent

- [ ] 新路由 `/api-keys/:id/overview` 注册；页面用 `useParams` 取 id，经 `GET /api/api-keys/:id` 拿 key name；detail 404/加载失败 → 错误态（提示 + 返回列表按钮）
- [ ] 各区块独立时间窗（RaceWindowControl + raceWindowBounds）；URL 支持 `period`/`offset`/`startTime`/`endTime` 初始窗，无参数默认当天（复用现有 overview 页 `initialWindowFromUrl` 模式）
- [ ] 顶部 6 指标概览卡：调 `stats/api-key-metrics`，复用 `MetricsSummaryCard` 渲染
- [ ] 调用分析折线：调 `stats/charts?apiKey=…`，复用 CallAnalysisCard / TrendLineChart
- [ ] Token 分析折线：调 `stats/charts?apiKey=…`，复用 TokenAnalysisCard / TrendLineChart
- [ ] 性能与可靠性分析四 Tab：调 `stats/insight?apiKey=…`，复用 InsightAnalysisCard
- [ ] i18n en / zh-CN 面板标题与区块标题（尽量复用现有 dashboard.* / race.* 键）
- [ ] 前端页面测试（mock detail + 各数据 hook + `useNavigate`）：断言顶卡/图表/错误态渲染、区块时间窗切换、URL 初始窗解析
