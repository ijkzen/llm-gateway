# spec.md — request_log_snapshot 统计快照

Status: implemented (2026-09-09，读路径端点等价测试全绿)

来源：`.scratch/request-log-snapshot/REQUIREMENTS.md`（2026-09-09，grilling 4 轮 + ponytail 门控）。决策记录：ADR-0021，词条：CONTEXT.md 统计域。

## Problem Statement

数据面板全部 12 个统计端点（summary / charts / insight / 5×rank / 4×metrics）每次查询都对 `request` 表做实时全扫聚合。`request` 表历史行落库即终态（ADR-0014），不会变化——反复聚合同一批不可变数据是纯重复劳动，库越大查询越慢。需要把「已闭桶的时间片」的聚合结果预计算落库，让查询只实时算快照未覆盖的增量。

## Solution

新增 `request_log_snapshot` 预聚合表（窄表/EAV：时间桶 × 统计主体 × 指标名 × 数值），把闭桶数据按四级时间粒度（小时/天/月/年）预先聚合；12 个端点读路径改为「闭桶读快照 + 未覆盖部分实时兑底聚合 request 表」，返回数字与今日口径一致。配套：每小时生成任务（幂等固化新闭桶 + 首启全量回填）、每小时自愈扫描任务（补漏跑桶）、时区变更自动全量重算。

## User Stories

1. 作为网关使用者，我希望打开数据面板任意窗口的折线/饼图/条形图/汇总卡，历史闭桶部分读快照、只有最近增量实时聚合，以便页面更快且随库增长不劣化。
2. 作为网关使用者，我希望快照尚未生成（首次部署前历史、停机期、跳跑任务）时各端点仍然返回与今天完全一致的正确数字，以便快照只是加速层、永不改变结果。
3. 作为网关使用者，我希望供应商/供应商模型/虚拟模型/API Key 的赛马表与指标卡同样享受快照加速，以便长窗口（月/年/全部历史）聚合不再全扫 request。
4. 作为网关使用者，我希望 insight 的调用/失败/token/吞吐/速率趋势走快照，以便性能分析页同样提速。
5. 作为网关使用者，我希望延迟分位数（p50/p90/p95/p99）数值与今天一致，以便闭桶时精确计算后入快照、不做近似。
6. 作为网关管理者，我希望看到 `stats_snapshot` 与 `stats_snapshot_rebuild` 两个内置定时任务，可查看日志、可手动执行（全量重建），以便运营与排障。
7. 作为网关管理者，我希望空流量闭桶也有全 0 哨兵行，以便图表补零正确、自愈能可靠发现漏跑桶。
8. 作为网关管理者，我希望改设置表时区后快照自动全量重算，以便桶边界与查询口径重新对齐、历史查询恢复快照加速。
9. 作为网关管理者，我希望快照指标主体键一律用表主键 id（api_key 也不例外），以便 Key 名称不再承担身份语义、展示名随时可 JOIN 解析。
10. 作为开发者，我希望读路径「快照缺失→实时兑底」与生成「幂等 upsert」有测试锁定，以便正确性不变量不回归。

## Implementation Decisions

1. **表结构（用户定义）**：`request_log_snapshot(id, duration_type, start_time, end_time, entity_type, entity, metric_type, metric_value)`，UNIQUE(duration_type, start_time, entity_type, entity, metric_type)。entity_type ∈ {whole, provider, model, virtual_model, api_key, virtual_model_member, api_key_model}；主体键一律 id（provider.id / provider_model.id / virtual_model.id / api_key.id）；跨维主体 entity = 逗号复合键（如 `"3,45"`）。迁移号 26。
2. **指标注册表（单一事实源）**：metric_type 清单 + 每个指标的口径（success/全量全集、populations 与各端点 SQL 完全同谓词）+ 类型（加和原语 / 计数 / 闭桶分位标量 / 哨兵）。存原语不存比率均值：cache 命中率、tps、avg、失败率、rpm/tpm、流式占比一律读时由原语现算。ratio 类指标在单桶内也可算（流式占比、失败率按桶计数现算）。
3. **生成路径**：一个 SQL 单遍扫闭桶 request 行产出全部主体模式行（GROUPING SETS；需要 provider_model 映射的 model/api_key_model/virtual_model_member 行经 JOIN 解析，映射不到的请求只进 whole/provider/virtual_model 等行）；分位标量（ttft/request_time 的 p50/p90/p95/p99）在 Rust 侧按「桶 × 主体模式」分组取原始值数组精确计算（仅 hour/day 行，月/年不存、接口语义返回空）。whole 哨兵行恒写（空桶全 0）。每桶整批一个事务。
4. **闭桶判定与读路径**：桶闭 = end_time + 60min ≤ now。任意窗口 [S,E)（含无粒度查询如 rank/metrics/summary）按「闭桶分解」：整闭桶 → 快照；边缘/未闭桶 → 递归降粒度（月→天→小时）到闭小时桶，最后实时聚合尾部。**缺失闭桶（无哨兵行）→ 实时兑底该桶**，不写穿（自愈任务后台补）。数字/补零/口径与现端点 SQL 恒等，由等价测试锁定。
5. **生成任务**（内置 `stats_snapshot`，@every 1h）：每次运行固化「自上次以来全部新闭桶桶」（四级各自判定；宕机后一次补多个）；首启（表空）整体全量回填整个 request 历史（按闭桶事务分片执行、幂等、可手动重跑，不做断点状态机）。
6. **自愈任务**（内置 `stats_snapshot_rebuild`，@every 1h）：最近 7 天缺哨兵行的闭桶小时/天桶 + 最近闭月/闭年点检，缺失即补算（复用生成路径）。
7. **时区**：桶边界 = 生成/查询时的设置表时区（ADR-0015 单一来源）；meta 表记录最近生成所用时区，启动或生成时发现与设置不一致 → 自动触发全量重算。
8. **失败原因分布不进快照**：insight failure_reasons 保持实时兑底（只扫失败行）；insight 分位趋势改为快照 p 标量行 + 未闭桶兑底。所有端点 JSON 形状不变。
9. **任务文案/种子/注册**：沿用 cron_jobs 种子行惯例（默认标题/描述 zh/en 各一），Handler 在 init 注册；日志用 JobLogLayer 自动捕获，满足排查需求。任务失败不得影响转发路径（错误仅记录）。
10. **快照行不设保留期清理**（request 表清理已撤销，快照为压缩数据）。

## Testing Decisions

- **生成器集成缝**：tests/ 集成（build_authed_app/内存库）造 request 行（跨桶边界、空桶、失败行、多供应商同名模型、api_key 存在/已删、迟到行、流式与非流式 ttft 分布），跑生成 → 断言快照行全集与哨兵行；重跑幂等不变；模拟中途失败事务原子性。先例：provider_boundary_probe_integration 等 tests/*_integration.rs。
- **读路径等价缝**：同一批 request 数据 → 全实时基准响应；再注入「部分闭桶快照 + 故意缺口/错位桶」→ 断言 12 端点响应与基准完全一致（含缺快照时兑底路径）；再补快照后断言仍一致（且不再走兑底——正确性+加速双证明）。先例：stats_integration（tests/stats/ 子模块）。
- **纯核心单测缝**：src 内闭桶判定/窗口→闭桶分解与实时尾部划分/指标注册表口径/分位计算/兑底归并等纯函数直测。先例：stats window/compute 单测、usage/estimate 纯核心（ADR-0008）。
- **cron 任务缝**：种子幂等（seed 惯例）、两个内置任务被调度/手动执行、自愈补缺场景、时区变更触发重算。先例：provider_quota_gate_integration 断言种子任务被调度。
- 前端零改动；既有 814 Rust + 417 前端用例全绿不回退。

## Out of Scope

- request 表任何改动；12 端点响应 JSON 与数字口径不变（等价测试锁定）。
- 分位数近似/直方图（闭桶精确计算，用户否决近似）。
- 月/年读时归并（用户否决，保留四级物化）。
- 回填断点续跑状态机（整体跑，幂等重跑）。
- 失败原因快照（结构不支持，实时兑底）。
- request/快照行保留期清理（已撤销/不设）。
- insight 月/年粒度分位（现语义即空，维持）。

## Further Notes

- 与 ADR-0015（窗口/时区单一来源）衔接：桶帧算术复用现有 ChartWindow/Granularity 纯函数，避免第二套边界实现。
- 自愈/时区重算期间读路径自动回到兑底，服务不中断。
- 规格落实后实现顺序：迁移+注册表纯核心 → 生成器（含分位）→ cron 任务 → 读路径逐端点改造 → 等价测试全绿。
