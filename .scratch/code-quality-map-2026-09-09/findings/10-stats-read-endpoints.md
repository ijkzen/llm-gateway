# FINDINGS · 10 stats 读端点审查（2026-09-10）

范围：`routes/stats.rs` 门面 + `stats/` 子目录（window/compute/summary_charts/insight/rank_impl/rank_snap/rank/metrics/tests，共 ~3400 行）+ 域内测试与 stats_snapshot_integration 等价测试覆盖面。方法：门面/window/compute/tests 会话内全读；summary_charts/insight/metrics 与 rank 族（rank/rank_impl/rank_snap）分两组后台子代理逐行深读（含与快照化前旧实现 `3cbad85~1` 的口径比对与前端 race hooks/图表契约核对）；头条候选逐条磁盘亲验（insight 分位跨层覆盖写、pm_rank 单侧过滤快照全量污染、删主体晚于固化丢行、SQL 手拼点）。等价测试已锁的「快照↔实时逐字节一致」本身不复查，只核其覆盖形态缺口。清单模式：不改代码——**两张 P1（10-01/10-02）曾按用户拍板插修并带回归验证（先红后绿），随后用户决定撤销代码改动回到纯审查**；修复方案与回归测试设计完整保留在本票（实施批指引），代码不在本图落地。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 10-01 | P1【插修已撤销，方案落实施批】 | 逻辑/分位 | insight day 粒度窗口含今日未闭天时，小时帧 p 标量覆盖写今日日桶分位（「过去 7 天」常见查询必现，尾点错）——group_percentiles 未校验帧级 == 查询粒度 |
| 10-02 | P1【插修已撤销，方案落实施批】 | 逻辑/过滤 | provider-model-rank 单侧过滤：仅 modelId 时快照 exact=None 读入全量 model 行混入其它供应商/模型（亲验）；providerId-only 被展示层 meta 的 provider 过滤兜住（响应正确但 fold 浪费）——修复=单侧过滤整窗兑底 |
| 10-03 | P3·语义【已拍板：接受现状+注释与测试锁定】 | 逻辑/删后时序 | pm/Key 硬删后（四个硬删端点已确认）闭桶历史快照态消失、兑底态按原文保留（pm_rank raw: 孤儿键即此设计）——「快照=加速层」在此时序不成立；快照行只存主键 id 无原文，schema 级统一成本高 |
| 10-04 | P3 | 逻辑/注入面 | api_key_rank 按 name 反查 id 用内联拼接（rank_impl.rs:723-727）——name 无字符集限制，含 `'` 即破坏 SQL；应改 `?` 绑定 |
| 10-05 | P3 | 健壮/契约 | PRIM_COUNT=9（rank_snap.rs:13）与 registry SuccessPrim 段序是运行期偶合——加/减原语不同步会在 fold 越界 panic，无 debug_assert 护栏 |
| 10-06 | P3 | 简洁/绕行 | charts 快照读用手写字面量 `&["calls","tokens_all"]`（summary_charts.rs:430/578/584）——summary/insight 已用常量数组，键改名会静默漏行 |
| 10-07 | P3 | 简洁/重复 | 逐 level 快照行折叠循环 ×5 处同形（summary/charts trend/model_distribution/insight/rank_snap）+ 桶 key 数学/补零输出 ×2——可收敛共享 helper |
| 10-08 | P3 | 测试覆盖 | api_key_rank 的 api_key_model 快照分支零等价测试；「今日未闭尾部」rank/metrics 等价缺（summary/charts 有）；空表/并列排序序/仅失败流量断言缺 |
| 10-09 | P3 | 健壮/风格 | insight p 标量快照读错误 `unwrap_or_default()` 静默吞（:467-469）退化为逐桶实时全扫——与同文件「Err 即响应」风格不一致 |
| 10-10 | P3·观察 | 微观察 | summary all_time 向兑底推 `(now, i64::MAX)` 未来段产生无意义扫描（:164-167）；window Granularity 字段在 30 天块缺省窗标 Hour（名实不符，内部）；charts/insight 单参窗口静默回退 24h vs summary 单参 400（旧行为非回归） |
| S5 | — | 归位遗留重估 | 同窗 ~12 次聚合：新结构已消解为每端点「快照 1 次批量取 + 兑底每段 1 条 GROUP BY（registry select_list 全指标单遍）」——**定案：已解决，关闭** |

## 各条证据

### 10-01 insight day 粒度分位尾桶污染（P1）【插修已撤销，方案落实施批】

insight.rs `group_percentiles`（:445-543）逐 level 读 p 标量（:462-477）时守卫只放行 `Hour|Day`（:464），**不校验帧级 == 查询粒度**；同桶多行按指标位**覆盖写**（:473-474 `entry[pos] = value`）。查询 granularity=day 且窗口含今日未闭天（典型「过去 7 天，end=now」）时：coverage 的 decompose(Day) 把今日部分天下钻为闭桶 Snap(hour) 帧（core.rs:264-273）→ by_level 含 Day 帧与 Hour 帧 → 小时帧 p 标量折入同一今日日桶索引（`(start+off).div_euclid(DAY_MS)`）互相覆盖 → 今日桶 p50/90/95/99 = 迭代序最后一个小时的 p；且 :479-488 因 `p_rows.contains_key` 跳过实时回算。:455 注释「跨层（细帧）不叠加」与行为相反（覆盖写同样错误）。窗口日界对齐（全部整闭日）才不触发——等价测试只用整闭的昨天，漏网。亲验：by_level 构建（:245-249）与覆盖计划（:216-240）确认跨层帧真实存在。

**处置（2026-09-10，两轮）**：首轮拍板立即插修并已实施（group_percentiles 只处理与查询粒度同层的帧，跨层细帧覆盖的尾桶落入「缺标量」分支整桶实时回算），回归测试 `insight_day_granularity_percentiles_equal_with_today_tail`（day 粒度+今日尾桶）验证先红（快照 p50=300 vs 实时 275）后绿；随后用户决定撤销代码改动回到纯审查清单模式。**实施批指引**：修复方案如上，回归测试形态如票内设计（等价测试补 day 粒度+今日尾桶，数据=昨日两条 ttft 100/300 + 今日两闭小时各一条 100/300 + 实时尾行 250）。

### 10-02 provider-model-rank 单侧过滤快照全量污染（P1）【插修已撤销，方案落实施批】

rank_impl.rs provider_model_rank（:232-397）：`supported`（:247）只排除 vm/apiKey；`exact` 要求 provider+model **同传**（:252-255）。单侧形态（仅 providerId 或仅 modelId）exact=None 且不 demote → 快照侧 `merged_prims(ENTITY_MODEL, None)` 经 fold_snapshot（rank_snap.rs:111-135）拉**全部 model 主体行**（所有供应商所有模型），兑底 grouped SQL（:265-281）经 push_rank_filters 只按单侧过滤。**亲验两种形态的实际表现**：modelId-only 是真污染——装配段 meta 查询（:300-313）无 provider 过滤时把全部 pm 键解析进展示，快照态响应混入其它供应商/模型（回归测试修复前红：混入供应商二 gpt-y）；providerId-only 恰好被同一 meta 查询的 `AND pm.provider_id = {p}`（:310-312）兜住——响应正确但 fold 浪费（每闭桶段读全量 model 行再丢弃）。注释（:242-243）自称支持「∅/providerId(/modelId 精确)」，单侧形态是漏网；stats_snapshot_integration 只测 ∅（:258/335）与精确（:550），单侧形态零覆盖。

**处置（2026-09-10，两轮）**：首轮拍板立即插修并已实施（单侧过滤 provider XOR model 按 unsupported 整窗兑底），回归测试 `provider_model_rank_model_filter_equality_with_snapshot` 验证先红（快照态混入供应商二 gpt-y）后绿；随后用户决定撤销代码改动回到纯审查清单模式。**实施批指引**：修复方案如上；回归测试形态=第二供应商 gpt-y 流量 + 前日闭桶 day 帧 + modelId-only（判别，修复前红）与 providerId-only（防回归）双断言。

### 10-03 删主体晚于闭桶固化：快照态丢行 vs 兑底态留行（P3·语义）【已拍板：接受现状+注释与测试锁定】

删主体操作确认存在且为硬删：provider_model（providers 路由 :599 delete_provider_model）、api_key（:197 delete_by_id）、provider（:628 级联删模型/成员）、virtual_model（:860 级联删成员），均无软删列、无快照表级联清理。删除时序差异：pm/Key 在闭桶**固化后**删除 → 快照行仍在但读侧映射不到：charts 分布展示名解析丢弃（summary_charts.rs:650-655）、insight/rank api_key 归并丢弃（subject.rs:118-122）、rank 枚举 meta 解析丢弃——历史流量从快照态消失；同窗兑底/纯实时按原文保留孤儿行（pm_rank 的 `raw:{providerId}|{modelId}` 键即孤儿保留设计，rank_impl.rs:265-272）。等价测试只覆盖「未删」与「精确过滤已删主体（demote）」两形态，删除时序未锁。**快照行只存主键 id 文本、无原文信息，schema 级孤儿保留需迁移 27 + 生成双键 + 全量回填，且与 09-09「不高估」修复方向相抵**。

**拍板（2026-09-10）**：接受现状。实施批=读侧注释化该时序差异（pm_rank raw 键保留 vs 快照丢的不对称在 subject/rank_impl 注释明示）+ 等价测试补一条「删主体后查含闭桶窗口」用例锁定现状语义（数字=快照丢、兑底留），防止未来误改。

### 10-04 api_key_rank name 内联拼接（P3，注入面）

rank_impl.rs:723-727 `SELECT id AS v FROM api_key WHERE name = '{name}'`——name 来自 request 表 api_key_name（创建时仅 trim 校验，无字符集限制，api_keys.rs:106-109），含 `'` 的名称会破坏/改变该 SELECT。值域系统自产、低危。默认解：改 `Statement::from_sql_and_values` + `?` 绑定（与文件内其余风格一致）。

### 10-05 PRIM_COUNT 运行期偶合（P3，健壮）

rank_snap.rs:13 `PRIM_COUNT = 9` + :36-62 九个下标访问器 + registry.rs:84-130 SuccessPrim 段序三方偶合：registry 段序是「过滤后枚举序」源头，fold_row 按**别名名**取值（抗列序漂移 ✓），但 Prims 数组下标仍由 success_prims() 枚举序决定——加/减原语不同步会在 fold 越界 panic 或口径错位，仅注释+等价测试约束（两侧共用同一 derive，错位时等价测试仍绿）。默认解：`debug_assert_eq!(PRIM_COUNT, success_prims().count())` 类守卫 + 访问器改由枚举序生成。

### 10-06 charts 快照指标名手写字面量（P3，绕行）

summary_charts.rs:430/578/584 快照读用手写 `&["calls","tokens_all"]` 字面量；summary 用 `SUMMARY_METRICS` 常量（:96 区）、insight 用 `SERIES_METRICS`（insight.rs:86-95）——指标键同一事实源（registry::metrics）下 charts 绕行，键改名会静默漏行（等价测试可兜但迟）。默认解：charts 快照名单并入常量（如 `CHARTS_SNAP_METRICS`）或复用 SUMMARY_METRICS。

### 10-07 读侧同构重复五处（P3，简洁）

「coverage.snapshots 按 level 分组 → 逐 level snapshot_rows → 按别名取值累加」循环同形出现在 summary（:172-197）、charts trend（:407-452）、model_distribution（:566-590）、insight fold_snapshot_rows（:244-265）、rank_snap fold_snapshot（:111-135）五处；桶 key 数学（`(ts+off).div_euclid`/period key）与补零输出在 charts（:511-538）与 insight（:284-314）各一份。默认解：抽共享 helper（fold_snapshot 泛化到任意指标名集合 + 按 entity 过滤），随 10-06 同批。

### 10-08 rank/metrics/insight 等价与断言缺口（P3，测试覆盖）

① api_key_rank 的 api_key_model 快照分支（rank_impl.rs:660-682，唯一手写 fold）零等价测试（等价只锁 ∅ 与 pm 精确）；②「今日未闭尾部 + 闭桶」混合 coverage 的 rank/metrics 等价缺（summary/charts 有 `summary_and_charts_equality_with_today_tail` 同形可参照——10-01/10-02 修复的回归锚即此形态）；③ 空表/单主体/并列排序序/仅失败流量（request_count==0 跳过）断言缺。默认解：随两张 P1 回归同批补 ①②。

### 10-09 p 标量读错误静默吞（P3，风格/健壮）

insight.rs:467-469 `snapshot_rows(...).await.unwrap_or_default()`——DB 读错误被吞为「无标量」，:479-488 对整窗闭桶逐桶实时全扫兜底（正确但掩盖故障 + 潜在 24 次全表扫描），与同文件其余分支「Err → db_error 响应」风格不一致。默认解：改 `?` 传播（闭桶缺标量仍走实时回算，读错误不应降级）。

### 10-10 微观察（P3）

① summary all_time 向兑底 live 推 `(now, i64::MAX)` 段（:164-167）——对 request 表未来范围的无意义扫描（旧 SQL 无上界语义的迁就），可改「无上界」专用路径或接受（正确性无损，每全量 summary 一次空扫）；② resolve_chart_window 无显式粒度 62d+ 窗口返回 granularity=Hour 但 bucket_ms=30*DAY_MS（window.rs:139-147）——granularity 字段名实不符（仅内部标记用，行为正确）；③ charts/insight 单参窗口静默回退 24h vs summary 单参 400——历史行为，非回归。

## 归位遗留项 S5 重估：同窗 ~12 次聚合【定案：已解决，关闭】

- **原状**（codebase-audit-2026-09-08 S5，P2）：insight 对同一时间窗发起 ~12 次独立聚合扫描（失败原因/五类汇总/分位逐值全量拉回），分位把窗口全部成功行逐值拉回 Rust 排序；month_mode 后置丢弃。旧证据行号 routes/stats.rs:727-834（单文件时代）。
- **重估（新结构）**：09-07 起读路径被 registry 单源化 + 桶迭代/快照兑底收敛重写——现状每端点 SQL 次数：**快照侧 1 次批量行取**（coverage 哨兵 1 次 IN 查询 + snapshot_rows 每 level 1 次 IN 查询）；**兑底侧每 live 段 1 条聚合 SQL**（summary 无分组单行、charts/insight 一条 `GROUP BY bucket` 携带全部 registry 表达式 select_list、rank/metrics 一条 `GROUP BY key` 携带九原语）——「同窗 12 次独立扫描」已消解为段数级的 1-3 条，且分位闭桶走快照标量（仅 hour/day 存储、缺标量桶才实时回算，不再整窗逐值拉回）；insight 失败原因整窗 1 条独立查询属不同构型（不可与系列合并）；month_mode 分位短路已前置（insight.rs:451-453）。
- **残余观察**：10-09（p 标量读错误静默降级可能触发整窗逐桶回算）、缺标量桶的逐桶实时回算（罕见，闭桶必有标量——仅生成滞后窗口）。月/年分位恒空为接口语义（registry percentile_level_ok），非扫描浪费。
- **定案**：S5 目标（同窗聚合合并单遍 + 分位免逐值拉回 + month_mode 前置短路）在新结构全部达成，**关闭**，不另行开票；残余为 10-09 单条。

## 已核验无问题区（避免后续票重复审查）

- **数字口径快照↔兑底等价**（除 10-01/10-02 两形态外）：summary/charts/insight/rank/metrics 快照侧=闭桶整桶聚合行加总、兑底侧=registry expr 逐段 SQL，两侧逐条对齐；比率类 tps/ttft/request_time「分子分母分别加总再除」由 rank_snap::derive 单实现（与旧 SQL 逐条比对一致）；分位/round_5/weighted_ratio 全委托 stats_snapshot::math 单源。等价测试锁定的形态无回归。
- **兑底 SQL 谓词单源**：全部走 registry select_list/success_prims/expr_of（rank_snap:17-22、insight:129 等），无手抄指标表达式；可变过滤全 `?` 绑定（filter_parts/params 序一致）；段边界为内部 i64 字面量。唯一例外=10-04（name 内联）。
- **Prims 抗列序漂移**：fold_row 按别名名取值、下标由 success_prims() 枚举序决定——live SQL 列序不参与功能耦合（10-05 只余数组长度/访问器偶合）。
- **删主体精确形态守卫**：demote_if_unresolved 在「非 whole + 键解析失败」整窗兑底（subject.rs:53-66），model/api-key metrics 与 pm_rank 精确形态有等价测试锁定（deleted_subject_filter_demotes_to_live）——09-09 修复无回归。
- **时区口径**：无 chrono::Local/客户端 tzOffsetMinutes 残余，全经 stats_tz_offset_minutes（window.rs:48-53，设置表时区窗口起点定偏移）；月/年补零经 natural_periods 全历法周期。
- **Top-N/折叠**：后端全量返回无折叠（前端 Top-10），排序稳定 + 并列预排确定序（rank.rs:66-73）。
- **前端图表契约**：insight/rank/metrics 响应结构有 race/统计集成测试锁定；空桶补零、删供应商空名等 UI 依赖形态已覆盖。

## 性能/内存轮结论

无 P1/P2（除 10-01/10-02 属正确性非性能）。正向：快照使闭桶段 O(1) 行取（S5 已消解，见归位段）；coverage 哨兵一次 IN 查询；trim_zero_prefix 裁初始化前空段；分位标量避免逐值拉回。残余：10-09（读错误降级触发整窗逐桶回算，事件性）、10-10-①（summary 全量未来段空扫，每请求一次）、summary all_time 无 start/end 时整窗兑底不可快照（全历史窗天然无闭桶快照覆盖之外的部分——trim 后窗口内闭桶仍走快照，正确）。结论：读路径成本 ∝ 帧数×行数不随请求总量涨，形态正确。
