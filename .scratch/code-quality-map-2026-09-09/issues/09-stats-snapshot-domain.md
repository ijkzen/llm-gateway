# 09 · stats_snapshot 快照域审查

Type: task
Status: claimed
Blocked by: 01

## Question

对统计快照域做全量审查：`stats_snapshot/` 全目录（core 桶帧/闭桶判定/窗口分解、registry 指标主体注册表、generator GROUPING SETS 生成、reader 兑底、subject 键解析、tasks 回填/自愈）+ 域内测试（含等价测试）。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：闭桶判定/哨兵行/分位标量口径、水位推进、时区重算、删主体降级（09-09 已修项不复查）、517 竞态修复（事务首写）后有无同型残余；
- 实现简洁：registry 谓词单一事实源是否被读侧绕行、生成器与读侧重复；
- 测试覆盖：generator_tests 之外缺什么（自愈补缺、回填幂等边界）；
- 模块间调用：与 10 读端点、db 迁移、cron tasks 的双向关系是否合适。

产出 `.scratch/code-quality-map-2026-09-09/findings/09-stats-snapshot-domain.md`，Answer 给摘要与需拍板问题。

## Answer

**清单产出**：`findings/09-stats-snapshot-domain.md`——8 条全 P3 无 P1/P2、无拍板项。方法=core/registry/reader/subject/mod/math 会话内全读 + generator/tasks/generator_tests 后台子代理逐行深读（本地实跑 8/8 测试绿 + generator 15 次连跑无抖动）+ 关键断言磁盘抽核（rowid 连接亲缘/day 测试名实相悖/时区重算窗口亲验）。

**核心结论**：快照域在图内属高度打磨域——**517 竞态修复无残余**（唯一事务 generator.rs:71 首语句=哨兵 16 行写，tasks 全路径无「事务内先读后写」）；**单写者收敛**（快照行/水位各唯一写者 + 进程锁串行，heal 与生成幂等重放无害）；09-09 修复项（删主体高估双守卫/insight year）无回归；registry 单一事实源无绕行；闭桶/水位/哨兵语义与 ADR-0021 一致。8 条发现集中三处：
- **测试健壮/名实**：seed_subjects 的 `last_insert_rowid()` 连接亲缘脆弱点（两次读跨连接取 p1/p2，现断言对任意 p1≠p2 自洽才绿——测试并未验证它声称的前提）；day 测试函数名「skips_percentiles」与断言（day 存分位 p50=250）相悖；mod.rs 死代码 allow + 陈旧注释（接入早已完成）。
- **测试缺口**：tasks 域（heal 只测 hour/未初始化让位/锁忙跳过/两 run 间新桶固化/时区重算中途失败再入/finalize 失败水位不前进 6 类）；generator 域（Year 无直测/分位 NULL-entity 排除无直测——删主体高估族的另一半/空 day 桶边界）。
- **观察级**：时区重算 DELETE→回填非原子窗口（失败到下次 run 前整段历史兑底，正确但慢，自愈靠偏移不匹配重触发）；增量水位遇反复失败不前移无退避（读侧兑底保正确）；空桶 16 行哨兵固定膨胀记账（与行量无关常数，读侧只依赖 calls 行）。

**需拍板问题**：无。本票无 P1/P2、无行为口径分裂点（自愈 7 天窗口/闭桶 60 分钟余量等均 ADR-0021 已拍板），8 条默认解明确直接落清单。

Status: resolved
