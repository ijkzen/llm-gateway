# REQUIREMENTS.md — request_log_snapshot 统计快照

> 来源：2026-09-09 start_work（grilling 4 轮 16 题 + ponytail 3 项精简提案，全部经用户拍板）。
> 核心动机：`request` 表历史行不可变，但所有数据面板查询（汇总/折线/饼图/条形/赛马/指标卡）每次都在实时全扫聚合——不必要。改为「快照表 + 增量实时聚合」：每个查询只需实时算快照未覆盖的增量部分。

## 1. 目标

1. 新建 `request_log_snapshot` 表，保存各时间维度（小时/天/月/年）闭桶桶的指标聚合。
2. 12 个 stats 读取端点（summary/charts/insight/5×rank/4×metrics）改为：**已固化整桶读快照，未覆盖增量实时聚合**，返回数字与今日实时口径一致。
3. 配套：快照生成定时任务 + 扫描重建（自愈）定时任务 + 首次全量回填 + 时区变更自动全量重算。

## 2. 表结构（用户定义，窄表/EAV）

```
request_log_snapshot
├── id             INTEGER 自增主键
├── duration_type  TEXT  'hour' | 'day' | 'month' | 'year'
├── start_time     INTEGER 桶起点（毫秒，本地时区边界，含）
├── end_time       INTEGER 桶终点（毫秒，本地时区边界，不含）
├── entity_type    TEXT  主体类型（见 §3）
├── entity         TEXT  主体键（见 §3）
├── metric_type    TEXT  指标名（registry，见 §4）
├── metric_value   REAL  数值
└── UNIQUE(duration_type, start_time, entity_type, entity, metric_type)
```

- 每桶 × 每主体实例 × 每指标 = 一行。同一条请求属于多个主体 → 进入多行。
- 指标值单列 REAL：所有存储值都是加和/计数原语（整数和 ≤ 2^53 内无损）或闭桶标量（分位数），不存比率/均值（读时现算，保证跨桶加总与现口径一致）。
- 新增指标只加行（新 metric_type 值），不加列。

## 3. 主体（entity_type × entity）

| entity_type | entity 语义 | 服务对象 |
| --- | --- | --- |
| `whole` | 空（全局） | 数据面板趋势/汇总、无过滤 insight |
| `provider` | provider.id | 供应商赛马/供应商过滤图/供应商指标卡 |
| `model` | provider_model.id（生成时由 (provider_id, model_id) 映射；映射不到的请求不计入 model 行，仍计入 provider/whole 等行） | 供应商模型赛马/分布图/模型指标卡 |
| `virtual_model` | virtual_model.id | 虚拟模型赛马/过滤图/指标卡 |
| `api_key` | api_key.id（生成时由 request.api_key_name 按唯一名映射；映射不到的请求——key 已删除——不计入 api_key 行，仍计入 whole/provider 等行） | API Key 赛马/过滤图/指标卡 |
| 交叉型：`virtual_model_member` | `"<vmId>,<providerModelId>"` 英文逗号分隔 | 虚拟模型页成员赛马、成员分布图 |
| 交叉型：`api_key_model` | `"<apiKeyId>,<providerModelId>"` | API Key 页模型分布图 |

- 交叉主体 = 现有主体的「交叉相乘」生成的新主体类型，entity 写 `1,2` 英文逗号分隔（用户定义）。
- 主体名（provider_name/api_key_name 等）读取时 JOIN 现表解析，与今日一致；已删除主体显示空名，行保留（主体键一律用 id：provider.id / provider_model.id / virtual_model.id / api_key.id，生成时由 request 行的名称/id 映射，映射不到的请求不计入该主体行）。

## 4. 指标注册表（metric_type，实现时逐端点 SQL 对账补齐）

计数/和原语（按各端点 SQL 的口径区分 success 全集与全量全集）：
`request_count`（成功）、`calls_total`（全部）、失败数、（流式数）、
token：`input_tokens`/`output_tokens`/`total_tokens`/`input_cache_tokens`（和的聚合）、
耗时：`ttft_sum`/`ttft_n`、`request_time_sum`/`request_time_n`、
tps：`tps_output_tokens`（Σ输出）/`tps_net_time_ms`（Σ output/tps，口径同 `tps_sql`）、
闭桶标量（**闭桶时对该桶原始值精确计算后存储，非近似**）：`ttft_p50/p90/p95/p99`、`request_time_p50/p90/p95/p99`（延迟分位仅 hour/day 行有意义；月/年行不存，接口现语义月/年分位返回空）。
比率/均值（cache 命中率、tps、失败率、avg ttft/request_time、rpm/tpm、流式占比）一律读时由原语现算。

**失败原因分布不进快照**（表结构无原因维度列）：insight 失败原因列表兑底实时（只扫失败行，量小）。

## 5. 读路径（核心不变量）

对任意窗口 [S, E) 与粒度 g（rank/metrics 无粒度 → 用「闭桶分解」）：
- 将窗口分解为：已固化整桶（快照）+ 边缘/未固化部分（实时聚合 request 表），数字合并且补零规则与现接口一致。
- **闭桶判定**：`bucket_end + 固化余量(60 分钟) <= now`。
- **第一类规则（用户强调）：快照未生成或未覆盖到的部分，一律实时取数**——缺失整桶（无 whole 哨兵行）也实时算该桶，不返回错误/空数据；快照只是加速层。
- 时区/桶边界：与现接口同一设置表时区口径；不匹配的旧桶自然落入「缺失→实时」。

## 6. 写路径：生成/自愈/回填

- **固化余量**：60 分钟（用户拍板）。超长流式（>60 分钟）跨过固化点的请求行会从对应小时桶快照漏掉（日/周/月视图由更粗粒度行覆盖，不受影响）。
- **四级独立物化**（用户拍板保留）：hour/day/month/year 各自直接对 request 表直算（不从细粒度行滚粗），每级独立哨兵/自愈。
- **空桶哨兵**：每个闭桶都写 whole 行（即使全 0）——「存在 whole 行」=「该桶已固化」，是自愈缺口检测的依据。
- 内置任务（cron_jobs 种子行，UI 可见、可手动执行）：
  1. `stats_snapshot`（@every 1h）：幂等 upsert 自上次运行以来新闭桶的桶（四级各按闭桶时刻判定，通常每次 1 个/级，宕机后补多个）；首启表空时整体跑全量回填（无断点状态机，幂等可重跑，用户拍板）。
  2. `stats_snapshot_rebuild`（@every 1h）：自愈扫描——最近 7 天缺 whole 哨兵行的闭桶小时/天桶 + 最近闭月/闭年点检，补算缺口。
- **时区变更自动全量重算**（用户拍板保留）：meta 记录生成所用时区，启动/生成时发现设置表时区变化 → 自动触发全量重算（复用回填代码路径，幂等）。
- 生成 SQL 采用 按主体模式各一遍的 GROUP BY 扫描（桶内行集小，功能与单遍等价且不依赖 SQLite 分组集支持）（覆盖全部 entity 模式 + 各指标原语 + 分位标量需原始值，分位标量在 Rust 侧分组取数组算）。
- 事务：每个桶的整批行一个事务，事务提交=该桶固化完成。
- 保留期：快照行不清理（request 表保留期清理已撤销，快照体积小）。

## 7. 非目标（ponytail 提案被用户否决保留 / 明确不做）

- 月/年读时归并（否决——保留四级物化）。
- 直方图近似分位数（不做——分位数闭桶精确算）。
- 断点续跑状态机（否决——整体跑，幂等重跑）。
- 失败原因快照（结构不支持，兑底实时）。
- request 表任何改动（只读它做聚合）；12 端点响应 JSON 形状不变（数字口径不变）。

## 8. 迁移

- 新迁移版本号 **26**（生产 schema_migrations 14/15 废弃号段，新迁移从 16 起编，勿撞号）。
- 含 snapshot 表 + snapshot_meta（键值）建表与索引。
