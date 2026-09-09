# 全仓代码质量审计地图：模块审查（清单模式，2026-09-09 立项）

## Destination

后端 + 前端全部功能域以模块为单位审毕：每张模块票产出该模块的一份四轴（逻辑正确性 / 实现简洁 / 测试覆盖 / 模块间调用）+ 专门性能与内存轮的分级审查清单（FINDINGS 式，P1-P3 分级、行号锚点逐一与磁盘现状复核），并带归位遗留项的重估结论。**清单模式：本图不实施任何代码改动**；改动在地图走完后统一另行排期。图走完 = 每个模块「改什么 / 为何 / 证据」已成决策文档，无悬而未决的审查问题。

## Notes

- **模式**：审出清单不改（wayfinder plan-only，执行不载入地图）。票内无歧义的问题直接进清单；需要用户拍板的（bug vs 设计如此、重构取舍）走 AskUserQuestion 当场记录结论，仍不改代码。P1 高危发现的修复时机由用户在票答覆盖时自行决定，图内默认不动。
- **审查轴**：四轴 + 每票一轮专门性能/内存审查。2026-09-08 已整改收官的范围不复查（`codebase-audit-2026-09-08/FINDINGS.md` 四维审计、`architecture-deepening-2026-09-08/REVIEW.md` C1-C7、架构评审轮 6 候选）；本票通读若发现新结构使其回归才重记。
- **清单格式**：沿用四维审计 FINDINGS 口径（编号 / 严重度 P1-P3 / 维度 / 一句话 / 证据行号），产出到 `.scratch/code-quality-map-2026-09-09/findings/<NN>-<slug>.md`，Answer 给摘要。行号引用必须逐条复核（教训：子代理报告是起点，执行前逐行核对）。
- **归位遗留项**：S3 request 表保留与 rollup → 03 票；S5 同窗聚合合并 → 10 票；usage fetcher 会话失效分类 → 06 票（均在对应票 Question 中显式列出）。
- **Tracker**：本地 markdown（`docs/agents/issue-tracker.md`）；ticket 均为 `task` 类型（agent 独立驱动），`Status: claimed/resolved` 认领后才动手，Answer 段记录结论。所有模块票被 01 盘点票阻塞。
- **范围权威**：各模块票的审查范围与切分以 `MODULES.md`（01 票产物）§1/§3/§4 为准——01 已解决（2026-09-09），其切分修正（08 补寄居文件、11 补路由文件、18 拆 21、19 缩小、hooks 冻结接口）已直接落进各票 Question。
- **Skills**：与用户拍板用 grilling（AskUserQuestion）；域术语若有冲突用 domain-modeling；无需 research/prototype。

## Decisions so far

- [01 · 模块边界全景盘点](issues/01-module-boundary-inventory.md)：全仓模块图（后端 20 模块两清分层、availability=纯 entity 底座、usage↔proxy 单点双向 by-design）+ 10 族散落候选（F3/F6 已单源免票，F1/F2/F4/F5/F7/F8/F9/F10 各有主票见 MODULES.md §4）+ 文件归属（failure_recovery.rs 寄居成立→08 审、failure_recheck 合理、usage 子模块干净）+ 前端切分修正（18 拆 21、19 缩小、16/17/20 注记）+ AGENTS.md 结构树漂移清单（§1.4，随实施批次刷新）。产物=MODULES.md。
- [02 · proxy 转发编排与选路审查](issues/02-proxy-forwarding-orchestration.md)：12 条清单（1 P2=终态失败零日志 / 11 P3），三项拍板：额度空候选 503 落库记失败行（含 NoMembers 补 warn）、直连成功清零保持现状只补注释、决策日志明细降 debug 留 info 选路结果。产物=findings/02-proxy-forwarding-orchestration.md。
- [03 · proxy 流式转运与指标记账审查](issues/03-proxy-stream-accounting.md)：9 条清单（1 P2=带内错误事件客户端假成功 / 8 P3），两项拍板：泵对转换器 error 态发 error 帧 + [DONE]（03-01 修复含 03-07 三协议回归；collect 非流式路径已正确可对照）、S3 request 表保留保持现状不清理（原 P1 读侧已被统计快照消除，登记体积观察项，复原形态存 revert 22bb5c8）。产物=findings/03-proxy-stream-accounting.md。
- [05 · proxy 协议转换审查](issues/05-proxy-protocol-conversion.md)：10 条清单（1 P3·安全观察 + 1 P3·口径 + 8 P3）无 P1/P2，96 例单测盘点；核心结论=历次审计打磨成熟面无回归（reasoning_details 全链路/思考三态×四协议/签名回传/finish 全表/usage 与 scanner 等价均核验）；新发现=复杂函数零测试三处（sanitize_gemini_schema/images 全路径/reasoning_details 上限截断）+ 死代码两处（collect_tool_call_names 计算即弃×2/GeminiStreamConverter.model 只写不读）+ json 模式 text 丢弃与多缓冲乱序 + Responses 回放全文保留内存观察；两项拍板：05-01 图片下载 SSRF+无界读保持现状记观察（信任 key 面，登记再评估条件）、05-03 直通思考开关双写保持现状+实证条目（DeepSeek 共存验证）；A7/C4/C5/B8/D3 旧账确认仍在随实施批。产物=findings/05-proxy-protocol-conversion.md。
- [06 · usage 厂商抓取层审查](issues/06-usage-fetcher-layer.md)：11 条全 P3 无 P1/P2 + 两项归位重估（13 fetcher 双子代理深读 + 关键断言磁盘抽核）；归位一重估出真问题=「3xx=过期 vs 401/403」分歧近乎空转（UsageHttp 未禁重定向，302→登录页被跟随成 200→Parse，6 家 3xx 守卫基本不可达），真分裂在 200 业务包络失效码（xiaomi 注释自认 code 401=登录态失效却落 Upstream，agentrouter/tokenrhythm/siliconflow/moonshot 明示失效被测试锁定 Upstream）→**拍板全面治理**（禁重定向+判定统一含 3xx+按家补包络 Auth 特征，四例锁定测试随批改，逐家实证）；归位二冷却**拍板**=商汤文案带剩余时间、krill 保持现状；单家新发现=MiniMax 首端点 401 短路不回退/火山错误信封中断 AFP 回退（均需实证）/AK-SK 签名错误判 Auth 倒挂×3/alibaba sec_token 裸拼 URL/krill 自愈失效形态/sensenova 冷却清除早于写回；测试面=HTTP 判定分支零单测 + 集成仅 6 条真链路（api_key 六家/alibaba/xiaomi/siliconflow/stepfun/copilot 零覆盖）；重复簇盘点（3xx×5/UA×2/round2×3 等，尊重不抽重复拍板）。产物=findings/06-usage-fetcher-layer.md。
- [07 · usage 持久化与额度门控审查](issues/07-usage-persist-gating.md)：5 条全 P3 无 P1/P2 无拍板（本域图内最干净之一：新鲜度判定真单一 cache_age_fresh* 全仓唯一、E7 原子 upsert 无回归、级联/manual-failure 守卫完备 15 测试、谓词单源三消费方共用 types 访问器、恢复双通道与 ADR-0010 自洽）；新发现=抓取入口无跨调用单飞+失败无负缓存（cron/手动/LB/recheck 四路并发重复抓，持续故障期 LB 每请求真实厂商调用含 DB 写失败期已计费放大，默认解=收敛 mem.fetch_shared 单飞或失败负缓存）/cron 刷新后 mem 持旧数据到自身 TTL（双写面不对称）/探活顺序执行无总时限/测试缺 mem 失败取消路径·read_usage_cache_many·probe_boundary 候选矩阵。产物=findings/07-usage-persist-gating.md。
- [08 · cron 调度与日志链路审查](issues/08-cron-domain.md)：1 P2 + 12 P3 + 归属定夺；核心=08-01 跨栈契约 P2：实时 SSE log 事件从不携带 seq（JobLogEvent 三发送点全 None）而 E6 防重设计与前端 `data.seq<=last.seq` 去重都建立在 seq 上——重叠窗口日志重复 + React key 恒 undefined，文档自认带 seq 与实现不符→**拍板全链路补 seq**（JobLogLayer per-span 捕获侧分配，广播 FIFO 与 DB flush 同源，Lagged 号段错位对去重安全，前端保留去重补单测）；09-08 已修 E1-E8/P1-P4 系列核验无回归；其余 P3=run_ended 先于落库的「永不结束」窗口（交换顺序）/worker 合成消息无 4096 截断/insert_log 死面/flush 失败丢批/6h 回收 failed→success 中间态/seed next_run_at=now 展示偏差/测试缺口九项；**归属拍板：failure_recovery.rs 移顶层 src/failure_recovery.rs**（唯一消费=cron 注册，与 availability.rs 平级）。产物=findings/08-cron-domain.md。
- [09 · stats_snapshot 快照域审查](issues/09-stats-snapshot-domain.md)：8 条全 P3 无拍板（高度打磨域：517 事务首写修复无残余[唯一事务首语句=哨兵写]、单写者收敛+进程锁、09-09 删主体高估双守卫/insight year 无回归、registry 单一事实源无绕行、闭桶/水位/哨兵与 ADR-0021 一致）；发现=测试健壮 seed_subjects last_insert_rowid 连接亲缘脆弱点（现断言对任意 p1≠p2 自洽才绿）+ day 测试名实相悖（名 skips_percentiles 实断言存分位）+ mod.rs 死代码 allow 陈旧注释；测试缺口 tasks 6 类（heal 只测 hour/时区重算中途失败再入/finalize 失败水位不前进等）+ generator 3 类（Year 无直测/分位 NULL-entity 排除无直测）；观察=时区重算 DELETE→回填非原子窗口/水位遇反复失败不前移/空桶 16 行哨兵膨胀记账。产物=findings/09-stats-snapshot-domain.md。

## Not yet specified

- 「P1 高危险发现」是否中途脱离清单模式插入修复（默认不改，图后统一排期）——由用户在票答覆盖时自行决定。
- 图后实施排期的组织方式（统一 backlog 文档？按严重度分批？按模块批量？）——终点之后的交付形态，图内不定。
- AGENTS.md 结构树刷新（漂移清单见 MODULES.md §1.4）与各票低危「口味级」条目的收敛决策——均归图后实施批次，图内不再开票。

## Out of scope

- 产品向工作：代际立项（adaptive thinking 等模型能力）、额度闸门恢复讨论、新厂商接入。
- 前端视觉/UX 评审（本图只审代码质量，不含设计走查）。
- 2026-09-08 及此前审计已整改项的复查（除非重构使其回归）。
