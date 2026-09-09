# 10 · stats 读端点审查

Type: task
Status: claimed
Blocked by: 01

## Question

对统计读端点域做全量审查：`routes/stats.rs` 门面 + `routes/stats/` 子目录（window/compute/summary_charts/insight/rank_impl/metrics）及域内测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：窗口契约三态解析、时区口径（设置表统一后有无残余）、分桶补零、Top-N 折叠边界、兑底路径与快照路径数字一致性（等价测试已锁的不复查）；
- 实现简洁：五 rank handler 是否仍抄写、insight 单 handler 体量、SQL 手拼面；
- 测试覆盖：rank/metrics/insight 单测之外缺什么；
- 模块间调用：与 09 快照域、request 表直读、前端图表契约的边界。

**归位遗留项 S5**：同窗 ~12 次聚合合并单 GROUP BY（09-07 起读路径被 registry 单源化/桶迭代收敛重写过，原语境已变）——在此基于新结构给出重估结论。

产出 `.scratch/code-quality-map-2026-09-09/findings/10-stats-read-endpoints.md`，Answer 给摘要与需拍板问题。

## Answer

**清单产出**：`findings/10-stats-read-endpoints.md`——2 P1 + 8 P3 + S5 归位定案（纯审查，代码零改动）。方法=门面/window/compute 自读 + summary_charts/insight/metrics 与 rank 族两组子代理逐行深读（含与快照化前旧实现口径比对与前端契约核对）+ 头条候选磁盘亲验。等价测试已锁形态无回归；时区设置表统一无残余；兑底 SQL 谓词 registry 单源；Prims 按别名取值抗列序漂移。

- **两张 P1（首轮拍板插修已实施并带回归验证，随后按用户决定撤销代码改动回到纯审查；修复方案与回归测试设计完整保留在 findings 供实施批指引）**：
  - **10-01 insight day 粒度分位尾桶污染**：group_percentiles 未校验帧级==查询粒度，day 查询下今日部分天下钻的小时帧 p 标量折入日桶覆盖写（「过去 7 天」必现，尾点错）。修复方案=只取同层帧，细帧覆盖的尾桶走整桶实时回算；回归测试 `insight_day_granularity_percentiles_equal_with_today_tail` 已设计验证（修复前快照 p50=300 vs 实时 275 红，修复后绿）。
  - **10-02 provider-model-rank 单侧过滤污染**：仅 modelId（无 providerId）时快照 exact=None 读入全量 model 行——修复方案=单侧过滤整窗兑底（回归测试已设计验证：修复前快照态混入供应商二的 gpt-y 红，修复后绿）。附注：providerId-only 形态被展示层 meta 查询的 provider 过滤兜住（响应正确但 fold 浪费）。
- **10-03 删主体语义（拍板：接受现状+注释与测试锁定）**：删主体硬删端点确认存在（provider_model/api_key/provider/virtual_model 四硬删）；pm/Key 闭桶固化后删除 → 快照态历史消失、兑底态孤儿保留（pm_rank raw: 键即孤儿设计）；快照行只存主键 id 无原文，schema 级统一成本高且与 09-09「不高估」方向相抵。
- **S5 归位重估（定案：已解决，关闭）**：新结构每端点 SQL = 快照 1 次批量取 + 兑底每段 1 条 GROUP BY（registry select_list 全指标单遍）；分位闭桶走标量免逐值拉回；month_mode 短路已前置——「同窗 12 次独立扫描」消解为段数级 1-3 条，残余=10-09 单条（p 标量读错静默降级可能触发逐桶回算）。
- **P3 族**：api_key_rank name 内联拼接改绑定（10-04）；PRIM_COUNT 运行期偶合加 debug_assert（10-05）；charts 快照指标名手写字面量并入常量（10-06）；读侧折叠/补零/桶 key 同构重复可收敛（10-07）；api_key_model 快照分支与今日尾桶 rank/metrics 等价缺口（10-08）；p 标量读错静默吞改 Err 传播（10-09）；微观察三则（10-10）。

**需拍板问题**：已全部当场拍板（10-01/10-02 插修方案已定并验证后撤销代码改动——纯审查，方案与回归测试设计落实施批；10-03 接受现状），无遗留。

Status: resolved
