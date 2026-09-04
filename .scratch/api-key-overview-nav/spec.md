# API Key 数据面板 — Spec

> Feature slug: `api-key-overview-nav`。由 `REQUIREMENTS.md`（grill-with-docs + ponytail 产出）合成，
> 按项目 issue tracker 约定存放于 `.scratch/api-key-overview-nav/`。
> 参考规格：`.scratch/provider-models/spec.md`、`.scratch/virtual-models/spec.md`；
> 已合入范例：`6405384`（provider-models 入口）、`dac8057`（virtual-models 入口）。

## Problem Statement

供应商模型页与虚拟模型页已能从列表直达各自「数据面板」（二级/三级详情统计页），并在区块标题、成员/模型卡片、
详情弹窗标题等处提供「名称 + 方向键」入口。API Key 列表页仍停留在纯管理表格：只能看到 key 的启停与掩码，
无法查看该 key 到底产生了多少调用、消耗多少 token、主要打向哪些供应商/虚拟模型/模型。用户希望 API Key 列表
也具备同规格的「跳转数据面板」能力。

## Solution

新增**独立的 API Key 数据面板**（按 key 维度聚合请求指标的二级统计页，形态对齐供应商/虚拟模型/模型详情页），
并从 API Key 列表页以「名称 + 方向键」进入。

- 路由 `/api-keys/:id/overview`（`:id` 为 API Key 数字主键）；页面经 `GET /api/api-keys/:id` 取 key name。
- 页面由若干**独立时间窗**区块组成：
  1. 顶部 6 指标概览卡（该 key：请求数 / 总 token / TTFT / 平均耗时 / TPS / 缓存命中率）。
  2. 调用分析折线（按时间桶）。
  3. Token 分析折线（按时间桶）。
  4. 性能与可靠性分析（失败 / 延迟分位 / Token 结构 / 吞吐 四 Tab）。
  5. 该 key 用到的**虚拟模型**排行（行点击跳 `/virtual-models/:id/overview`，携带窗口）。
  6. 该 key 用到的**供应商**排行（行点击跳 `/providers/:id/overview`，携带窗口）。
  7. 该 key 用到的**模型**排行（行点击跳 `/models/:providerId/:modelId/overview`，携带窗口）。
- key 不存在（已删除 / id 非法）→ 页面显示错误态并引导返回列表。

## User Stories

1. 作为网关管理员，我想在 API Key 列表点击某个 key 的名称（带方向键）进入它的数据面板，以便对齐供应商模型/虚拟模型页的跳转体验。
2. 作为网关管理员，我想在 key 数据面板顶部看到该 key 在所选时间窗内的请求数、token 总量、TTFT、平均耗时、TPS、缓存命中率，以便快速判断该 key 的用量与性能水位。
3. 作为网关管理员，我想按区块切换时间窗（天/周/月/年/自定义，各区块独立），以便分别观察不同指标的趋势而不互相干扰。
4. 作为网关管理员，我想看到该 key 的调用量随时间的折线，以便发现流量突增/突降。
5. 作为网关管理员，我想看到该 key 的输入/输出 token 随时间的变化，以便评估成本与缓存效果。
6. 作为网关管理员，我想看到该 key 的性能与可靠性分析（失败趋势 / TTFT 与耗时分位 / Token 结构 / 吞吐），以便定位慢请求与失败。
7. 作为网关管理员，我想看到该 key 主要打向哪些**虚拟模型**（含各 6 指标），以便了解该 key 的业务入口分布。
8. 作为网关管理员，我想看到该 key 主要打向哪些**供应商**（含各 6 指标），以便核对账单与供应商用量。
9. 作为网关管理员，我想看到该 key 实际命中的**供应商模型**（含各 6 指标），以便细查具体上游模型表现。
10. 作为网关管理员，我想在以上三张排行卡中点击某一行直接跳转到对应虚拟模型/供应商/模型的数据面板，并保持当前时间窗，以便逐层下钻分析。
11. 作为网关管理员，当某个 key 已被删除时，我希望它不再出现在列表（自然无入口），且旧 URL 打开面板显示明确的错误态，而不是白屏或错数据。
12. 作为网关管理员，我希望 name 列的箭头跳转不干扰同行的启停开关与行尾菜单（点名称/箭头才跳转）。
13. 作为网关管理员，我希望从列表点击进入时若带时间窗参数，页面各区块能按该时间窗初始化（与首页/详情页赛马深链一致）。

## Implementation Decisions

### 后端（`src/routes/stats.rs`）

为 stats 查询统一引入可选的 `apiKey` 过滤参数（字符串，精确匹配 `r.api_key_name`）。所有端点沿用现有
「`where_sql` 字符串 + `params: Vec<sea_orm::Value>`」公共模式追加 `AND r.api_key_name = ?`。各端点新增
`api_key: Option<String>` 入参（`ChartsQuery`/`RankQuery` 或各自 Query struct 上加字段，serde camelCase）。

需要支持 `apiKey` 过滤的端点：

| 端点 | 用途 | 改动 |
|---|---|---|
| `GET /api/stats/charts` | 调用/Token 折线 | `ChartsQuery` 加 `api_key`，charts() where 追加 |
| `GET /api/stats/insight` | 性能可靠性四 Tab | 与 charts 共用 `ChartsQuery`，insight() where 追加 |
| `GET /api/stats/provider-rank` | 该 key 用到的供应商排行 | `RankQuery` 加 `api_key`，provider_rank() 追加 |
| `GET /api/stats/virtual-model-rank` | 该 key 用到的虚拟模型排行 | 同上，virtual_model_rank() 追加 |
| `GET /api/stats/provider-model-rank` | 该 key 用到的模型排行 | 同上（已支持 provider_id），追加 api_key |
| `GET /api/stats/api-key-metrics`（**新增**） | 顶部 6 指标卡 | 新端点：入参 `apiKey`+时间窗，单行聚合（仿 model_metrics 无 GROUP BY），WHERE 加 `r.api_key_name = ?` |

约束：

- `apiKey` 缺省 = 不过滤（向后兼容：现有调用方不传，行为不变）。
- 过滤值精确匹配 name（非 LIKE、非前缀）。
- 排行卡行深链所需的 providerId / modelId / virtualModelId 本就在各 rank 返回行内，无需改响应结构。
- insight 响应的 `apiKeyRank`（现用于 hasTraffic 判定）在加了 `api_key` 过滤后只含该 key 单行或空；
  其「数组长度 > 0」的判空语义不受影响（该 key 有请求即长度 1），不把它当全量聚合消费。

### 前端

- **新 hook**：`web/src/hooks/use-api-key-metrics.ts` —— 对齐 `useProviderMetrics`/`useVirtualModelMetrics`，
  请求新端点 `stats/api-key-metrics?apiKey&startTime&endTime`，返回 6 指标（复用 `MetricsData` 形态）。
- **新页**：`web/src/pages/api-key-overview.tsx` —— 骨架对齐 `virtual-model-overview.tsx`：
  - `useParams` 取 `:id` → `useApiKeyDetail(id)` 拿 name；detail 未加载/404 → ErrorState + 返回列表。
  - 各区块独立 `RaceWindowState`；`initialWindowFromUrl` 从 URL query 初始化（默认当天）。
  - 复用组件：`MetricsSummaryCard`（顶卡，title 可自定义）、CallAnalysisCard / TokenAnalysisCard /
    InsightAnalysisCard（图表区块）、provider-overview 内联排行表形态抽成可传 apiKey 的复用块
    （或就地实现三张小表；倾向抽一个 `RaceTable`-like 的本地小表避免过度抽象——实现时定）。
  - 三张排行卡分别调 `useProviderRace` / `useVirtualModelRace` / `useProviderModelRace` 并透传 `apiKey`，
    行点击 `navigate` 到对应面板（携带该区块窗口参数，复用现有 `openProviderOverview` 式 query 拼装）。
- **列表入口**：`web/src/components/api-keys/ApiKeysTable.tsx` name 单元格改为「名称 + ChevronRight」导航区，
  点击 `navigate(/api-keys/${id}/overview)`。形态对齐 ProviderModelCard 的 `data-nav` span（hover 底色、
  箭头微移），不引入整行可点。行内启停 Switch、行尾菜单保持原样。
- **路由**：`App.tsx` 在 `api-keys` 路由旁加 `<Route path="/api-keys/:id/overview">`。
  不加侧边栏项。
- **i18n**：en / zh-CN 新增面板标题、区块标题（复用现有 dashboard.* / race.* 键，尽量不新增文案）。

### 时间窗 & 深链参数

区块时间窗用现有 `RaceWindowControl` + `raceWindowBounds`。URL 支持 `period`/`offset`/`startTime`/
`endTime` 初始窗（对齐 `initialWindowFromUrl`，provider/virtual-model/model overview 页已实现，直接复用该模式）。
排行行深链拼装 query 时：custom 窗带 start/endTime，否则带 period/offset —— 与
`InternalModelRaceTable.openModelOverview` 一致。

### 已删除 key 的语义

`GET /api/api-keys/:id` 404 → 页面 ErrorState（提示 key 不存在，按钮返回列表）。不做历史查看。
（用户拍板：key 删除后历史不可查。request 行按 name 留痕，但按 id 路由时删除即不可达，避免同名重建混杂。）

## Testing Decisions

好测试 = 验证**外部行为**：给定若干带不同 `api_key_name` 的 request 行，断言某端点加 `apiKey` 过滤后
只返回该 key 的数据；给定 key 列表页渲染，断言名称区点击触发导航且启停仍可操作；给定 key 面板 URL，断言
detail 404 时渲染错误态。

| Seam | 模块/文件 | 验证点 |
|---|---|---|
| 后端 stats 集成测试 | `tests/stats_integration.rs` | 现有 charts 有 provider 过滤用例，追加 `apiKey` 过滤用例（两个 key 各若干行 → 断言只聚合目标 key）；insight / provider-rank / virtual-model-rank / provider-model-rank 各补一个 `apiKey` 过滤用例；新增 api-key-metrics 端点用例（有/无请求、跨 key 隔离、参数缺失 400） |
| 后端 race 集成测试 | `tests/provider_race_integration.rs`、`tests/virtual_model_race_integration.rs`、`tests/provider_model_race_integration.rs`、`tests/virtual_model_member_rank_integration.rs` | 如属同批改动就近补充 |
| 前端页面测试 | `web/src/__tests__/api-key-overview-page.test.tsx`（新）、`api-keys-page.test.tsx` | 新面板：mock detail + 各 hook，断言顶卡/图表/排行渲染、detail 404 → ErrorState、区块窗口切换；列表页：name+箭头点击 `navigate` 被调用、启停 Switch 仍工作（mock react-router `useNavigate`，仿 dac8057 测试） |
| overview 结构测试 | model-overview/provider-overview/virtual-model-overview 现有 page test 模式 | 面板页沿用「区块独立窗口 + URL 初始窗」结构测试 |

Prior art：
- 后端 `stats_integration.rs::test_charts_with_window_and_provider_filter`（窗口+provider 过滤断言）。
- 前端 `virtual-models-page.test.tsx` / `provider-models-page.test.tsx` 中 `vi.mock("react-router-dom")`
  注入 `useNavigate`、`fireEvent.click` 断言编程跳转（dac8057）；`api-keys-page.test.tsx` 现有表格测试。

## Out of Scope

- 不做单 key 的请求日志表区块（request-logs 已支持 apiKey 多选过滤）。
- 不做面板内启停/删除 key 的管理操作。
- 不做 key 删除后历史查看（不可查，用户拍板）。
- 不新增侧边栏项 / 不改 overview 首页与其它详情页（本页只加新端点过滤参数，不要求现有页消费）。
- 不为 summary / api-key-rank 加 apiKey 过滤（未在本页展示，YAGNI）。
- 不引入新依赖；图表/排行/指标卡全部复用现有组件。

## Further Notes

- request 表 `api_key_name` 为字符串留痕，API Key 的 name 唯一约束（`src/entity/api_key.rs`）。
- `MetricsSummaryCard` 接受 `MetricsData` 六指标切片，api-key-metrics 响应直接映射该类型，无需新渲染组件。
- 排行卡/图表后端改动是「同一 where 模式加一个可选参数」，各端点改动同构；前端 hook 逐一透传新参数。
- api-key 数据面板标题建议含 key name（detail 返回）+ 现有 `dashboardPage.titleSuffix`。
