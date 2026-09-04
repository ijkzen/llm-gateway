# API Key 数据面板（跳转数据面板功能）— Requirements

> 本文件由 start_work 流程产出：Stage 1 grill-with-docs（用户逐卡拍板）+ ponytail 收敛。

## 来源

用户原话：「按照供应商模型和虚拟模型的规格，给 API Key 页面也添加到跳转数据面板的功能。」

参考规格：`.scratch/provider-models/spec.md`、`.scratch/virtual-models/spec.md`，以及已合入 main 的
`6405384 feat(provider-models): add overview navigation entry points` 与
`dac8057 feat(virtual-models): add overview navigation entry points to match provider-models`。

## 目标（Scope）

新增**独立的 API Key 数据面板**（二级页，仿供应商/虚拟模型/模型详情页形态），并在 API Key 列表页提供
入口。核心差异：request 历史只按 `api_key_name`（字符串）聚合，无按 id 反查路径。

### 路由

`/api-keys/:id/overview`，`:id` 为 API Key 数字主键。

- 页面挂载后先取 `GET /api/api-keys/:id` detail（含明文 key 的实体：id/name/enable/createdAt…）拿 name。
- 拿不到（key 已删除 / 不存在）→ 错误态（ErrorState + 返回列表），不做 404 之外的特殊处理。
- **key 删除后历史不可查**（用户拍板）：删后 detail 404，列表无入口，直接 URL 也打不开面板。
  语义与 request 表按 name 聚合天然冲突（name 可重建），但按 id 路由时删除即 404，无同名混杂问题。

### 列表页入口

- API Key 列表行 **name 列变为“名称 + ChevronRight 方向键”**，点击编程跳转 `/api-keys/:id/overview`。
  - 完全对齐 dac8057/6405384 的「模型 ID + 方向键 data-nav 区」交互：点击名称/箭头才跳转，
    不干扰同一行的启停 Switch 与行尾操作菜单。
- API Key 列表是 DataTable（非区块卡片），只把 name 单元格做成可点击导航区，行其余部分不变。

### 数据面板内容（用户逐卡拍板，全部保留）

页面骨架与 provider-overview / virtual-model-overview 同构：`PageHeader`（Key 名 + 后缀）+ 若干独立时间窗区块。

1. **顶部 6 指标概览卡**（该 key 窗口内：请求数/总 token/TTFT/耗时/TPS/缓存命中率）。
   - 后端新增 `GET /api/stats/api-key-metrics?apiKey=<name>&startTime&endTime`（单行聚合，无 GROUP BY，
     仿 model_metrics，WHERE 加 `r.api_key_name = ?`）。
   - 前端用与 MetricsSummaryCard 一致的数据形态渲染（无供应商/虚拟模型关系，仅数字卡）。
2. **调用分析折线**：复用 `useDashboardCharts` + CallAnalysisCard，charts 后端加 `apiKey` 可选过滤。
3. **Token 分析折线**：复用 `useDashboardCharts` + TokenAnalysisCard，同一次后端过滤。
4. **性能与可靠性分析（四 Tab）**：复用 `InsightAnalysisCard`，insight 后端加 `apiKey` 可选过滤。
5. **「该 key 用到的虚拟模型」排行表**：复用 provider-overview 的排行表形态，行点击跳
   `/virtual-models/:id/overview`（携带当前时间窗）。
   - 后端 `virtual-model-rank` 加可选 `apiKey` 过滤。
6. **「该 key 用到的供应商」排行表**：行点击跳 `/providers/:id/overview`。
   - 后端 `provider-rank` 加可选 `apiKey` 过滤。
7. **「该 key 用到的模型」排行表**：行点击跳 `/models/:providerId/:modelId/overview`。
   - 后端 `provider-model-rank` 加可选 `apiKey` 过滤。

时间窗口：**各区块独立**（每个区块自己的 RaceWindowControl + raceWindowBounds，与现有详情页一致）。
URL 支持从列表点击时携带一次初始窗口 query（period/offset/startTime/endTime，对齐
`initialWindowFromUrl`）；无 query 默认当天。

### 侧边栏 / 路由注册

- 新路由加在 App.tsx `<Route path="/api-keys/:id/overview" ...>`（`api-keys` 路由旁，仅注册不导航）。
- 不新增侧边栏项（数据面板从列表页进入，与 provider/virtual-model 详情一致）。

## 非目标（ponytail 裁剪）

- **不做** 单 key 的请求日志表区块 —— request-logs 页已支持 `apiKey` 多选过滤，面板内不重复列表。
- **不做** 面板内对该 key 的「启停/删除」操作 —— 数据面板只读，管理仍回列表页。
- **不新增** 后端 summary / api-key-rank 的按 key 过滤 —— 顶部指标用专用 api-key-metrics，
  排行反查用 rank + apiKey；不为未展示数据改接口。
- **不做** key 删除后的历史查看（见上，用户拍板）。
- 不引入任何新依赖；图表/卡片/时间窗全部复用现有组件。

## 开放问题（已解决）

| 问题 | 结论 |
|---|---|
| 跳转目标 | 独立 API Key 数据面板（用户选定，非 request-logs 预过滤） |
| 入口形态 | name 列名称+箭头，编程跳转 |
| 顶部指标卡 | 要 |
| 三个趋势/分析区块 | 全要（调用/Token/性能四 Tab） |
| 三张排行卡 | 全要，行点击深链各自面板 |
| 路由标识 | 按 id（/api-keys/:id/overview） |
| 删除后历史 | 不可查（入口随删除消失 + URL 404） |
| 时间窗口 | 各区块独立 + URL 携带初始窗 |

## 边界与风险

- request 只存 name：后端所有新过滤统一按 `api_key_name` 精确匹配（`r.api_key_name = ?`），
  非 LIKE。
- api_key_rank（赛马卡片）在后端 insight 里按 GROUP BY api_key_name 聚合；本面板不需要它作为过滤目标，
  但 insight 的 hasTraffic 判定依赖 apiKeyRank 数组长度 —— 加 apiKey 过滤后 insight 的 apiKeyRank
  会变成仅含该 key 的单行（长度 1），hasTraffic 判定不受影响（>0 即可），需注意别把该字段当「全量」。
- 排行卡时间窗与自身区块绑定，深链跳转带该区块窗口。
- 删除中的 key（并发）：detail 404 → ErrorState 引导返回，无竞态特殊处理。
