# FINDINGS · 09 stats_snapshot 快照域审查（2026-09-10）

范围：`stats_snapshot/` 全目录（core.rs 540 行 / registry.rs / reader.rs / subject.rs / mod.rs / math.rs 会话内全读；generator.rs / tasks.rs / generator_tests.rs 由后台子代理逐行深读并本地实跑 8/8 测试绿 + generator 15 次连跑无抖动）+ 与 db 迁移 26、lib.rs cron 注册/预热、routes/stats 读侧（10 票域，只核接口）交叉核对。已知边界：09-09 架构轮修复项（insight year 恒零、删主体快照高估）、517 竞态修复（7daf6d7 事务首写）核验**无回归**（generator 事务首语句=哨兵 16 行写，tasks 全路径无「事务内先读后写」残余；删主体双守卫=聚合 NULL 键跳过 + 分位 row_entities 排除，r5 测试锁定；时区重算/水位/哨兵语义与 ADR-0021 一致）。清单模式：不改代码。**本票无 P1/P2、无需要拍板项**（全部 P3 默认解明确）。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 09-01 | P3 | 测试健壮 | generator_tests seed_subjects 用两次 `SELECT last_insert_rowid()`（:138-139）取两个 provider 的 id——跨连接读是连接局部的，p1/p2 不保证对应两次 INSERT；现断言恰好对任意 p1≠p2 自洽才绿，池路由变化即脆 |
| 09-02 | P3 | 简洁/nit | `day_level_bucket_skips_percentiles_and_crosses_hours`（generator_tests.rs:551）名实相悖——day 存分位且断言 p50=250（:610-614 注释「天行存分位」），函数名是复制遗留 |
| 09-03 | P3 | 简洁/死开关 | mod.rs 遗留 `#![allow(dead_code)]` 与两处 `#[allow(unused_imports)]` + 陈旧注释（「读路径/任务接入前」「端点接入中」）——全部已接入，allow 掩盖真实死项 |
| 09-04 | P3 | 测试覆盖 | tasks 域缺口：heal 只测 hour（day/month/year 补算、未初始化让位、锁忙跳过 Ok(false) 无测）；「两 run 间新闭桶在第二次固化」无直测；时区重算中途失败再入（半删态）无测；finalize 失败→水位不前进→重跑补齐无测 |
| 09-05 | P3 | 测试覆盖 | generator 域缺口：Year 级无直测（与 month 同分支无独立断言）；分位 NULL-entity 排除路径（row_entities pm/ak 缺失）无直测（聚合侧 r5 锁定、分位侧仅空样本间接覆盖）；空 day 桶哨兵数边界无测 |
| 09-06 | P3·观察 | 逻辑/时区重算窗口 | tasks.rs:101-115 时区重算先 DELETE 全表再逐桶回填——中途失败到下次 run（最长 1h）整段历史走实时兑底（正确但慢）；自愈依赖偏移不匹配持续触发，非永久损坏 |
| 09-07 | P3·观察 | 逻辑/退避 | tasks.rs:120-137 增量水位：某闭帧 finalize 反复失败（坏行/磁盘）则该级水位永不前移、每次 run 从旧水位重扫失败、无退避——读侧逐桶兑底保正确 |
| 09-08 | P3·记账 | 存储/膨胀 | 每桶哨兵 16 行（whole 全 0）× 4 级 × 空桶也写——长期固定开销 ≈ 16 × 桶数（与 request 行量无关），量级记账确认 |

## 各条证据

### 09-01 generator_tests seed_subjects 的 last_insert_rowid 连接亲缘（P3，测试健壮）

generator_tests.rs:138-139：连续两次 `INSERT INTO provider` 后各跑一次 `SELECT last_insert_rowid()`（经 scalar_i64，独立 acquire）取 p1/p2。sqlx 连接池的 `last_insert_rowid` 是**连接局部**的——两次读若路由到不同连接（写连接 + 读连接）返回 (0, 第二个 rowid) 之类错位值；现测试恰好「p1/p2 任意互异赋值都自洽」（provider_model 无 FK、后续断言只依赖 p1≠p2 的取值一致性）才全绿。池路由行为一旦变化（如两次读都落在写连接 → p1==p2），provider 主体断言（:353-356 期望 4 行合流）即红——**测试在验证一个它并未真正建立的前提**。同函数内 api_key/model 都用 WHERE name 反查 id（:150-154/:170-171），唯独 provider 依赖连接亲缘。默认解：p1/p2 改为 `SELECT id FROM provider WHERE name='p1'/'p2'` 反查，与同函数其余写法对齐。

### 09-02 day 测试函数名与断言相悖（P3，nit）

generator_tests.rs:551 `day_level_bucket_skips_percentiles_and_crosses_hours`——函数体注释与断言明确 day **存**分位（:610-614 `TTFT_P50=250`，注释「天行存分位（day 粒度接口要逐桶分位）」），与 registry `percentile_level_ok`（hour/day 存）一致。函数名疑为 month 版复制遗留——读测试名会得出「day 不存分位」的错误结论（与 10 票读端点契约相关，易误导）。默认解：改名 `day_level_bucket_stores_percentiles_and_crosses_hours`。

### 09-03 mod.rs 死代码 allow 与陈旧注释（P3，简洁）

mod.rs:14 `#![allow(dead_code)] // 读路径/任务接入前，纯核心与生成器先落地供直测` + 两处 `#[allow(unused_imports)] // 端点接入中，部分面暂未消费`（:28/:42 附近）——reader/tasks/routes 全部已接入（stats_snapshot_integration 等价测试逐字节一致在跑），注释所述阶段早已过去。allow 会掩盖未来的真实死代码/未用导入。默认解：删除 allow 与注释，让编译器暴露实际未用项（若有真未用再逐项定夺删除或保留理由），编译+clippy 验证。

### 09-04 tasks 域测试缺口（P3，测试覆盖）

tasks.rs 4 例（首启回填+增量不重复/自愈重建/时区重算/并发写者 517 回归）覆盖面好但缺：
① heal 只测 hour 缺失重建；day/month/year 补算分支、未初始化让位（:179-181）、锁忙跳过（Ok(false)）均无测；
② 「两次 run 之间新闭桶在第二次被固化」无直测（现有测试只验证重复 run 不新增）；
③ 时区重算中途失败再入（半删行/半水位状态）无测（09-06 的自愈依赖此路径正确）；
④ finalize 中途失败→水位不推进→下轮重跑补齐无测（水位推进在整级循环后 meta_set，:129-137——该不变量值得一条失败注入测试锁定）。默认解：按①②③④补 4 条（④ 可用坏行注入或 mock finalize 失败）。

### 09-05 generator 域测试缺口（P3，测试覆盖）

generator_tests.rs 4 例覆盖 7 主体全指标/幂等/空桶哨兵/day-month 分位差异，缺：
① Level::Year 无直测（year 走与 month 同分支=16 行无分位，无独立断言）；
② 分位 NULL-entity 排除路径（row_entities 的 pm/ak 缺失分支，generator.rs:193-205）无直测——聚合侧有 r5（pm 已删）锁定，分位侧只被空样本间接覆盖；删除主体的分位高估是 09-09 修过的高估族，值得分位侧直测钉死；
③ 空 day 桶哨兵数与「仅某主体有样本、其余主体无分位行」边界。默认解：补 3 条。

### 09-06 时区重算非原子窗口（P3，观察）

tasks.rs:101-115：偏移不匹配 → `DELETE FROM request_log_snapshot`（:107）→ 删 4 水位 → full_backfill → 置 tz meta。DELETE 与回填之间（含回填中途失败）快照表为空/半空——读侧对闭桶缺哨兵逐桶兑底（reader.rs:88-96），语义始终正确但整段历史走实时 SQL，直至下次 run 重触发（最长 1h；重触发条件=tz meta 未更新，偏移不匹配持续成立，天然自愈）。真实大库（60k+ 行回填实测分钟级）下该窗口的读性能代价需接受；替代（保留旧行直到新行就绪的换表式重算）需磁盘双份，不值。观察级，产品可接受；实施批可在时区变更前经设置接口提示「将全量重算」。

### 09-07 增量水位遇反复失败不前移（P3，观察）

tasks.rs:120-137：`finalize_bucket` 失败经 `?` 中止整级，水位（meta_set 在循环外 :137）保持旧值——若某闭帧持续失败（坏行/磁盘满），每 run 都从旧水位重扫并失败，无退避/跳过语义。读侧对该桶兑底保正确，代价=每 run 一次失败扫描 + 该桶后新闭桶延迟固化（水位被卡）。观察级；默认解=单桶失败记 warn 并继续后续帧（水位推进到失败帧前一帧）——需权衡「水位越过错桶后自愈不再覆盖」：heal 7 天窗口可兜近段，>7 天错桶需人工。随实施批评估，倾向保持现状（失败中止=最保守）。

### 09-08 空桶哨兵 16 行膨胀记账（P3，存储）

每闭桶事务首写 whole/''/0.0 × 16 指标哨兵行（generator.rs:73-75）——空桶（无请求）也固定 16 行/桶/级。长期固定开销 ≈ 16 × Σ各级桶数（hour 24/天 × uptime 天 + day 1/天 + month + year），与 request 行量无关；年化估算：1 年 uptime ≈ 16×(8760+365+12+1) ≈ 146k 行固定哨兵（hour 占大头）+ 有数据桶的主体行。量级与现有 request 全史行数相比小，但「空桶也写 16 行」在 hour 级是纯膨胀（哨兵谓词只需 calls 1 行，:221-238 bucket_finalized 只查 calls）。记账确认即可；若未来压缩=哨兵只写 calls 行 + 事务首写语义不变（需评估 16 行 whole 全 0 哨兵是否被读侧依赖——读侧只依赖 calls 行存在性，聚合缺失指标由空 SUM 回落 0.0 语义补齐，倾向可压缩，随实施批验证等价测试）。

## 已核验无问题区（避免后续票重复审查）

- **517 竞态修复贯彻（无残余）**：本域唯一 `db.begin()` 在 generator.rs:71，事务首语句=16 条哨兵写（:73-75），聚合 SELECT 全在写之后——事务升级点在无读快照的干净点，结构性不可能 BUSY_SNAPSHOT；tasks 全部 meta 操作/时区 DELETE 为自动提交单语句，无「事务内先读后写」；prune（快照域无 prune，属 log 域）不适用。并发回归由 tasks.rs:392 `generation_survives_concurrent_request_writes`（multi_thread 文件库 + 并发写者 + 3 轮重造首启）锁定。
- **09-09 修复项无回归**：删主体高估=聚合 NULL 键跳过（generator.rs:99-100）+ 分位 row_entities 双守卫（:193-205），r5 测试锁定（generator_tests.rs:407-421）；insight year 恒零族读侧无回归（10 票域等价测试在跑）。迁移 26 唯一索引（db.rs:552-553）与 upsert 冲突目标一致，upsert 幂等测试锁定（generator_tests.rs:497-505）。
- **单写者收敛**：快照行唯一写者=finalize_bucket 的 upsert_row（生成/回填/自愈三入口 + 时区全表 DELETE 均被同一进程锁串行，tasks.rs:15/84/175）；水位唯一写者=run_snapshot_generation（full_backfill 与增量分支互斥，heal 不碰）；heal 先固化 → 生成 run 幂等重放无害。跨进程多实例部署时进程锁失效（SQLite 写锁+busy_timeout 兜底但 517 防护假设单写者）——单实例模型下无此问题，多实例属未来架构项。
- **registry 单一事实源无绕行**：生成端 SQL 文本来自 METRICS 表（generator.rs:73/76/113），读侧 select_list/expr_of 同源（10 票消费）；无任何直接手写指标名绕行点；哨兵谓词单定义（reader.rs demote_missing/bucket_finalized 同模块内两句同形 SQL，可抽一——轻微，随 09-03 清理批）。
- **闭桶/水位语义**：闭桶谓词唯一（core.rs:52-54 end+margin）；水位粒度=各级最近固化闭帧 start（重启读回、缺失锚定 latest_closed_start 回看）；finalize 幂等使「水位推进前崩溃→重跑」安全；空桶天然只有哨兵行、读侧按 0 补。core 纯函数（frames/decompose/natural_periods）测试充分（11 例含闰年/跨年/边缘/荒谬守卫）。
- **模块间接口**：tasks 经 lib.rs 两个 cron handler（@every 1h）+ 启动 warmup（共享同一进程锁，任务运行时预热自动跳过）；与 db 迁移 26 的 upsert 前提一致；10 读端点只消费 Coverage/快照行/指标表（读侧合并留在端点，本域只保证判定唯一来源）——双向关系干净。
- **自愈范围**：7 天窗口 + 最近 2 闭月/闭年与 ADR-0021 决策 6 一致；7 天前缺桶不补由读侧兑底兜住（设计边界非缺陷）。

## 性能/内存轮结论

无 P1/P2。正向：覆盖计划一次 IN 查询查齐哨兵（reader.rs:62-98，避免帧级逐查退化）；快照行批量取（snapshot_rows 帧集 IN + 指标 IN）；分位样本收集单遍扫描排序（生成时一次性，读侧 O(1) 取标量）；水位推进使增量 run 只固化新闭桶；trim_zero_prefix 裁剪初始化前空段避免全史兑底扫描（reader.rs:180-217）。P3 级：09-08（空桶 16 行哨兵固定膨胀——与行量无关的常数，记账确认）、09-06（时区重算窗口内整段历史兑底——事件性）。结论：读路径成本 ∝ 闭桶帧数 × 帧内行数（不随请求总量涨，记忆已证），形态正确，无需结构性改动。
