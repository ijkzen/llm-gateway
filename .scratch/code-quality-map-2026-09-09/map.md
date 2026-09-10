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
- [04 · proxy 上游传输与探活审查](issues/04-proxy-upstream-transport.md)：13 条全 P3 无 P1/P2 无拍板（本图迄今最干净模块，09-08 P1-P6 整改无回归）；发现=复用重试靠 total_ms()==0 隐式判定/陈旧连接双重计费窗口无注释、IPv6 字面量主机不可用且报错误导（http 1.4.2 Uri::host() 保留方括号实证）、probe/metrics 两处注释漂移、UpstreamCall.stream 与 connect_done_at_ms 死字段、探活前奏三处重复、test_model 成功行 request_id 断链、四组超时不可注入零覆盖、failure_recovery probe_gate 绕过 10 分钟缓存每时真抓（归口 06）、手动测速路由无超时无取消（最坏 ~4 分钟）；性能轮=每 host 连接无上限+突发滞留 600s 观察级。产物=findings/04-proxy-upstream-transport.md。
- [05 · proxy 协议转换审查](issues/05-proxy-protocol-conversion.md)：10 条清单（1 P3·安全观察 + 1 P3·口径 + 8 P3）无 P1/P2，96 例单测盘点；核心结论=历次审计打磨成熟面无回归（reasoning_details 全链路/思考三态×四协议/签名回传/finish 全表/usage 与 scanner 等价均核验）；新发现=复杂函数零测试三处（sanitize_gemini_schema/images 全路径/reasoning_details 上限截断）+ 死代码两处（collect_tool_call_names 计算即弃×2/GeminiStreamConverter.model 只写不读）+ json 模式 text 丢弃与多缓冲乱序 + Responses 回放全文保留内存观察；两项拍板：05-01 图片下载 SSRF+无界读保持现状记观察（信任 key 面，登记再评估条件）、05-03 直通思考开关双写保持现状+实证条目（DeepSeek 共存验证）；A7/C4/C5/B8/D3 旧账确认仍在随实施批。产物=findings/05-proxy-protocol-conversion.md。
- [06 · usage 厂商抓取层审查](issues/06-usage-fetcher-layer.md)：11 条全 P3 无 P1/P2 + 两项归位重估（13 fetcher 双子代理深读 + 关键断言磁盘抽核）；归位一重估出真问题=「3xx=过期 vs 401/403」分歧近乎空转（UsageHttp 未禁重定向，302→登录页被跟随成 200→Parse，6 家 3xx 守卫基本不可达），真分裂在 200 业务包络失效码（xiaomi 注释自认 code 401=登录态失效却落 Upstream，agentrouter/tokenrhythm/siliconflow/moonshot 明示失效被测试锁定 Upstream）→**拍板全面治理**（禁重定向+判定统一含 3xx+按家补包络 Auth 特征，四例锁定测试随批改，逐家实证）；归位二冷却**拍板**=商汤文案带剩余时间、krill 保持现状；单家新发现=MiniMax 首端点 401 短路不回退/火山错误信封中断 AFP 回退（均需实证）/AK-SK 签名错误判 Auth 倒挂×3/alibaba sec_token 裸拼 URL/krill 自愈失效形态/sensenova 冷却清除早于写回；测试面=HTTP 判定分支零单测 + 集成仅 6 条真链路（api_key 六家/alibaba/xiaomi/siliconflow/stepfun/copilot 零覆盖）；重复簇盘点（3xx×5/UA×2/round2×3 等，尊重不抽重复拍板）。产物=findings/06-usage-fetcher-layer.md。
- [07 · usage 持久化与额度门控审查](issues/07-usage-persist-gating.md)：5 条全 P3 无 P1/P2 无拍板（本域图内最干净之一：新鲜度判定真单一 cache_age_fresh* 全仓唯一、E7 原子 upsert 无回归、级联/manual-failure 守卫完备 15 测试、谓词单源三消费方共用 types 访问器、恢复双通道与 ADR-0010 自洽）；新发现=抓取入口无跨调用单飞+失败无负缓存（cron/手动/LB/recheck 四路并发重复抓，持续故障期 LB 每请求真实厂商调用含 DB 写失败期已计费放大，默认解=收敛 mem.fetch_shared 单飞或失败负缓存）/cron 刷新后 mem 持旧数据到自身 TTL（双写面不对称）/探活顺序执行无总时限/测试缺 mem 失败取消路径·read_usage_cache_many·probe_boundary 候选矩阵。产物=findings/07-usage-persist-gating.md。
- [08 · cron 调度与日志链路审查](issues/08-cron-domain.md)：1 P2 + 12 P3 + 归属定夺；核心=08-01 跨栈契约 P2：实时 SSE log 事件从不携带 seq（JobLogEvent 三发送点全 None）而 E6 防重设计与前端 `data.seq<=last.seq` 去重都建立在 seq 上——重叠窗口日志重复 + React key 恒 undefined，文档自认带 seq 与实现不符→**拍板全链路补 seq**（JobLogLayer per-span 捕获侧分配，广播 FIFO 与 DB flush 同源，Lagged 号段错位对去重安全，前端保留去重补单测）；09-08 已修 E1-E8/P1-P4 系列核验无回归；其余 P3=run_ended 先于落库的「永不结束」窗口（交换顺序）/worker 合成消息无 4096 截断/insert_log 死面/flush 失败丢批/6h 回收 failed→success 中间态/seed next_run_at=now 展示偏差/测试缺口九项；**归属拍板：failure_recovery.rs 移顶层 src/failure_recovery.rs**（唯一消费=cron 注册，与 availability.rs 平级）。产物=findings/08-cron-domain.md。
- [09 · stats_snapshot 快照域审查](issues/09-stats-snapshot-domain.md)：8 条全 P3 无拍板（高度打磨域：517 事务首写修复无残余[唯一事务首语句=哨兵写]、单写者收敛+进程锁、09-09 删主体高估双守卫/insight year 无回归、registry 单一事实源无绕行、闭桶/水位/哨兵与 ADR-0021 一致）；发现=测试健壮 seed_subjects last_insert_rowid 连接亲缘脆弱点（现断言对任意 p1≠p2 自洽才绿）+ day 测试名实相悖（名 skips_percentiles 实断言存分位）+ mod.rs 死代码 allow 陈旧注释；测试缺口 tasks 6 类（heal 只测 hour/时区重算中途失败再入/finalize 失败水位不前进等）+ generator 3 类（Year 无直测/分位 NULL-entity 排除无直测）；观察=时区重算 DELETE→回填非原子窗口/水位遇反复失败不前移/空桶 16 行哨兵膨胀记账。产物=findings/09-stats-snapshot-domain.md。
- [10 · stats 读端点审查](issues/10-stats-read-endpoints.md)：2 P1+8 P3+S5 定案；**两张 P1（10-01/10-02）已实施修复（2026-09-10，commit d8f7825，含回归测试与 ADR-0021 补 Decision 8/9）**——修复方案与回归测试设计保留在 findings：10-01 insight day 粒度分位尾桶污染（小时帧 p 标量折入日桶覆盖写，「过去 7 天」必现，修=只取同层帧/尾桶实时回算）、10-02 provider-model-rank 仅 modelId 快照读入全量 model 行混入他供应商（providerId-only 被展示层兜住，修=单侧过滤整窗兑底）；10-03 删主体晚于固化语义拍板=接受现状+注释与测试锁定（硬删四端点确认，快照行只存主键 id 无原文、schema 级统一与 09-09 不高估方向相抵）；**S5 定案=已解决关闭**（新结构每端点 快照 1 次批量取+兑底每段 1 条 GROUP BY registry 全指标单遍，分位走标量免逐值拉回，12 次→1-3 条）；P3=api_key_rank name 内联拼接/PRIM_COUNT 运行期偶合无守卫/charts 快照指标名手写字面量/折叠补零同构重复×5/api_key_model 快照分支零等价/读错静默吞/微观察。产物=findings/10-stats-read-endpoints.md。
- [11 · CRUD API 域审查](issues/11-crud-api-domain.md)：27 条（3 P2+24 P3）+两项拍板+F9 定案；三 P2=11-01 时区变更 reload 先于缓存刷新用旧时区重建（与自身注释矛盾、backup 路径顺序正确对照；当前种子任务全时区不敏感零可见影响，有 tz 敏感 cron 即升 P1）/11-02 用量缓存失效 vs 在途抓取无版本护栏（脏缓存最长一个 TTL 自愈）/11-03 request_logs 分页 offset u32 溢出（dev 500/release 静默错页）；**F9 定案=无注入面**（request_logs 全绑定+白名单、providers.rs 的 format! 实例已自然消亡、stats 侧只插 i64 常量；三套 WHERE 构造器并存属简洁收敛项）；**拍板一=VM 成员不加供应商可用性校验**（成员关系≠可用性，路由层剔除兜底）、**拍板二=全局 5MB body 上限保持**（DoS 兜底，巨型多模态 413 接受）；P3 要点=多步写非原子×2/未加载 cron 任务永不可删/校验缺口族（custom_header 非对象、备份 apiKey 必填绕过、enable⇔reason 不变式无校验、Bool 不 trim）/缓存面（删设置键不刷缓存、备份导入留孤儿缓存）/性能（usage map N+1 有现成批读、用量端点无单飞=07-01 同解、无分页列表、索引慢路径）；测试缺口 14 类（时区重载/SSE 后端零集成/竞态族/i18n 英文裸面等）。产物=findings/11-crud-api-domain.md。
- [12 · 鉴权与中间件域审查](issues/12-auth-middleware-domain.md)：9 条全 P3 无 P1/P2 无拍板（1048 行小域主代理直读）；发现=init check-then-act 并发双初始化可建两用户/Cookie 无 Secure（观察）/mask bytes 判长 chars 切片多字节重叠泄漏/decrypt_or_passthrough 对解不开的密文也透传当明文（与写侧教训不对称）/logout 不公开过期会话清不掉 cookie/v1-messages 前缀无边界/lang 取值旁路+TTL 字面量重复/api 双点查微观察；测试缺口六类（并发双 init、过期会话、x-api-key 入站、cookie 属性、auth 英文分支、logout 无会话）；已核验 argon2id+时序均衡 dummy/会话与 Bearer 面（key_hash 索引、双错误形状）/守卫层序（CatchPanic 最外兜 auth）/CORS 维持已登记口径/crypto 五错误路径测试齐/依赖方向无反流。产物=findings/12-auth-middleware-domain.md。
- [13 · 数据与实体层审查](issues/13-data-entity-layer.md)：6 条全 P3 无 P1/P2 无拍板（db.rs 1202 行+entity 16 文件直读）；发现=并发首启迁移撞车 fail-fast（注释过度声称）/迁移 1 else 分支丢 changed 致 ANALYZE 可跳过/双轨 schema 收敛只靠纪律（schema_check 只覆盖 provider；snapshot_meta updated_at 类型名漂移实例 affinity 相同无害）/**db.rs 超行评估=采纳测试迁出**（558/1202 行是 cfg(test)，迁 src/db/tests.rs 有 cron 先例纯搬运，主链 454 行不再拆）/新库死列 17 加 22 删抖动/测试缺口四类（并发迁移需文件库、坏库半迁移、迁移 1-12 老库模拟、回滚路径）；已核验版本守卫事务化+两坑注释化有撞号回归、连接配置单处、实体零行为纯 schema。产物=findings/13-data-entity-layer.md。
- [14 · 供应商数据管理域审查](issues/14-provider-data-domain.md)：16 条（1 P2+15 P3）+F1/F5/F8 三家族定案；**唯一 P2=14-01（F8 升级为真实 bug）**：build_models_url 版本段白名单窄于 build_upstream_url，9 个 v3/v4 种子供应商（火山/腾讯/Z.AI/智谱/Eden）刷新模型必 404——火山 v3 修复只修推理侧漏刷新侧；F1 定案=枚举无 parse/PartialEq 反向接口致字面量绕行依旧（无新增，补接口全量替换）、F5 定案=死码枚举×4+裸数字范围 12+ 处（同仓有常量区间正面样板）；P3 要点=首启空库时区分叉（cron Local vs timezone_sync 上海，重启自愈）/max_consecutive_failures i64 校验 vs u32 解析越界静默/**测试 harness scheduler 与 app 各建独立 AppSettings（11-01 测试不可见根因）**/模板 extra 回填五份近重复+更新分支不回填/模板 upsert 按 name 无删除留孤儿/recover_quota 不清失败计数口径待钉死/availability 样板×4+逐行 UPDATE；已核验 ADR-0003 状态机无漂移、读侧谓词唯一、模板回填只补缺、无反向依赖。产物=findings/14-provider-data-domain.md。
- [15 · 系统工具与基础单体域审查](issues/15-system-utility-modules.md)：12 条（1 P2+11 P3）+一项拍板+handler/seed 双源对账 4/4；**P2=优雅关停名存实亡**（with_graceful_shutdown 无超时，SSE 日志流/流式 /v1 长连接钉死 serve→scheduler.stop 与 worker 10s 收尾永不执行→SIGKILL 兜底硬杀 in-flight 任务，与 lib.rs 注释承诺相反；默认解=timeout 包 serve）；**拍板=APP_ENV 非法值静默回退 Dev 保持现状**（生产拼错起空库事故面已知悉，有测试固化）；P3=备份成员唯一性不校验撞索引报裸 SQL/导出解密失败静默空串（与 11-13 串联空凭据备份链）/settings 导出无排序/未知设置类型降级 String/导入不清 request 表历史悬空（并 11-21）/AGENTS.md example handler 文档漂移/i18n 插值两形态/static_assets 三微/db_error 直返 DbErr Display 观察/测试缺口六类（build_export 无单测、关停零回归）。产物=findings/15-system-utility-modules.md。**后端 02-15 十四票全部审毕。**
- [16 · FE 数据面板与日志域审查](issues/16-fe-dashboard-logs.md)：17 条（7 P2+10 P3）无拍板（前端首票，双子代理+全 P2 复核）；七 P2=?period=foo 非法参数穿无 default switch 链整页 ErrorBoundary/virtual-model 与 provider 概览页缺 detail 404 错误态（另两页有守卫）/请求日志时间窗口变更不重置 page 越界假死/4 列头可排序不在后端白名单静默回退/insight 图例硬编码中文（tooltip 已英文）/X 轴桶标签用浏览器时区与设置表错位/吞吐图 RPM/TPM 同轴量级失配；**01 盘点两骨架候选定案=抽 MetricRaceCard+AnalysisSections 区块层不抽整页（不触 hooks 冻结接口）、insight 语义色不统一**；P3=自定义窗口时区口径/ApiKeyRaceCard 缺 initialWindow/CSV 生成侧无转义/inferGranularity 无 year 未爆弹/赛马表全量渲染/测试缺口族（initialWindowFromUrl 真实函数未被测=往返测试是复刻版）。产物=findings/16-fe-dashboard-logs.md。
- [17 · FE 配置管理域审查](issues/17-fe-configuration-management.md)：28 条（6 P2+22 P3）无拍板+弹窗脚手架定案；六 P2=ProviderDetail 切供应商在途明文 Key 串号/用量开关一去不返（可见性绑 usage===true 而非存在键）/useMatchTemplate 404 吞不掉（await 在 try 外+无防抖）/toast 用不存在的 i18n key/ProviderDetail 无保护 JSON.parse（密文透传整页崩）/Add 弹窗 manual/pending 候选卡整卡可点致数字输入死控件（测试全 fireEvent.change 是无回归网根因）；**弹窗脚手架定案=只抽 DialogScrollShell 布局原语（19 落地 17 消费，ADR-0004 需 addendum），不抽表单装配**；P3=代理 @ 校验口径/明文 Key 进 query 缓存策略相悖/useDeleteProvider 漏失效 providerModelKeys/详情弹窗滚动违规×2+refetch 丢编辑态/VM 不可解析草稿成员仍提交/假防抖/三处近重复/测试缺口族。产物=findings/17-fe-configuration-management.md。
- [18 · FE 任务/设置域审查](issues/18-fe-cron-settings-auth.md)：20 条（4 P2+16 P3）无拍板；四 P2=**18-01=08-01 前端半边实证**（log 事件无 seq 去重失效[undefined 比较恒 false]+React key 恒 undefined+测试 emitLog 注入 seq 假阳性契约，08-01 落地自愈）/18-02 reset 整体替换竞态（丢增量+staleTime 命中旧缓存回退）/18-03 设置表直改 language 不热切换（正路 useChangeLocale 只 locale-toggle 在用，前后端长期分叉）/18-04 SettingEditDialog 类型盲（Int/Bool/Float 纯文本无校验）；P3=SSE idle 不刷 runs/立即执行 1s 刷新拿不到 last_run_at/删除弹窗 onError 关窗不一致/日志 O(n²) 渲染/SSE 无退避 401 永久重连中/Json 编辑静默丢数据/导入文件残留/改密字符 vs 字节口径/测试缺口两族；**i18n 锚点汇总归 20 票**（settings 五文件+17 两实例，zh-CN.ts:545-564 既有键未被用可直接复用）。产物=findings/18-fe-cron-settings-auth.md。
- [19 · FE 共享组件与基础设施域审查](issues/19-fe-shared-infrastructure.md)：22 条（4 P2+18 P3）无拍板（双子代理+ky node_modules 实证）；四 P2=sidebar cookie 只写不读刷新必回展开/**网络栈三兄弟**：beforeError 换 ApiError 类型致 ky retry 白名单失效（4xx 也重试+Retry-After 被绕）+NETWORK_ERROR 分支不可达（网络故障 toast 显示英文原文）+全局 10s 超时短于用量上游 15s（前端先失败后端实成功）；P3=MidEllipsis 强制重排×46 实例+零宽渲染「…」/multi-select 硬编码 id//login 懒加载在 Suspense 外/双 retry 叠加 4 次/401 丢 from 回跳/api.ts+ui+data-table 零测试/时区键三处定义+PAGES 死码/manualChunks 漏 react-router 核心/setup 全局 mock 掩盖真实时区 hook；已核验依赖单向/chart 无注入/分页契约/constants 对齐。产物=findings/19-fe-shared-infrastructure.md。
- [20 · FE 全局文案与国际化域审查](issues/20-fe-i18n-global.md)：10 条（2 P2+8 P3）无拍板；**键集合脚本实证：zh/en 各 703 键零缺零多零占位符不匹配**（被引用未定义仅 apiKeys.showKeyFailed=17-04）；两 P2=html lang 首帧不跟随（languageChanged 注册晚于 init 同步触发漏首帧，i18next 实证；一行修）+zh-CN 的 dashboard.success/failed 写英文值（中文界面 tooltip 英文与同图中文图例矛盾，与 16-05 合流修）；P3=死键 160/703（usage/time 整组全死）/手写日期格式绕 i18n 而 time.* 全组死键/DEFAULT_GROUP 硬编码「默认」/全局 i18n.t 非响应式直调易碎/无制度化校验（一致性用例可抓 17-04 类）/i18n 初始化靠隐式副作用/language 只写不读（18-03 同族）；硬编码新实例=constants+race-period 9 处+dashboard-charts 3 处+utils 计数单位观察。产物=findings/20-fe-i18n-global.md。
- [21 · FE 会话与演示域审查](issues/21-fe-session-demo-domain.md)：4 条（1 P2+3 P3）无拍板（891 行小域直读）；P2=**初始化保存竞态**（saveInitSettings 在 init mutation 启动即发 PUT，argon2 延迟保证 PUT 先于会话建立→401→首启时区静默丢失+成功路径弹保存失败 toast；修=移到 onSuccess）；P3=chat 吞 03-01 流内 error 帧（无 choices 被忽略，内容戛然而止）/eventData 单行 data 假设观察/测试缺口族；已核验 RequireAuth 双通道、chat 主链路、use-auth 面。**至此 21 票全部审毕，地图走完。**产物=findings/21-fe-session-demo-domain.md。

## 实施进度

**状态（2026-09-10 收尾）**：全部 **2 条 P1 + 31 条 P2 已修复并提交**，文档逐条标记；
P3（约 230 条）按用户决定留待后续排期，本轮不动。6 次提交：
`d8f7825`（P1 两条）→ `92cd523`（02-01/03-03/08-01/21-01）→ `384b402`
（11-01/11-02/11-03/11-25/14-01/14-05）→ `7674205`（17-01~17-05 + 19-02~19-04）
→ `4863c53`（16-01~16-07 + 17-06 + 15-01 + 12-01）→ `a5f8b83`
（03-01/03-02 + 18-01~18-04 + 19-01 + 20-01/20-02）。
质量门：后端 `cargo test --all-targets` 853 passed；前端 vitest 429 passed；
`cargo fmt --check` / `clippy -D warnings` / `biome check` / `tsc --noEmit` 全绿。

- **03-01/03-02（P2/P3）已修复**：带内错误事件补发 error 帧（假成功消除）+ [DONE] 后 teardown 噪音按成功结束。
- **18-01~18-04（P2）已修复**：SSE seq 前端生效、reset 合并、language 热切换、设置弹窗类型控件。
- **19-01（P2）已修复**：sidebar cookie 读回（刷新保持折叠）。**20-01/20-02（P2）已修复**：html lang 首帧 + zh dashboard 英文值。
- **16-01~16-07（P2）已修复**：period 白名单、两概览页错误态+门控、日志分页重置、四列禁排序、insight 图例 i18n（并修 zh 的 success/failed 英文值）、X 轴标签走设置表时区、RPM/TPM 拆轴。
- **17-06（P2）已修复**：候选卡数字输入 stopPropagation（manual/pending 字段可填）。
- **15-01（P2）已修复**：HTTP 优雅关停加 8s 上限，长连接不再钉死 worker 收尾。
- **12-01（P3）已修复**：init 双初始化竞态改原子「表空才插入」+ 并发回归测试。
- **17-01/17-02/17-03/17-04/17-05（P2）已修复**：明文 Key 在途串号、用量开关一去不返、模板匹配 404 未吞+无防抖、不存在的 i18n key、extra/customHeader 无保护 JSON.parse。
- **19-02/19-03/19-04（P2）已修复**：beforeError 破坏 ky 重试白名单、网络/超时错误英文原文、全局超时短于后端；新增 api.ts 单测。
- **11-01/11-02/11-03/11-25（P2/P3）已修复**：时区重载顺序、用量缓存失效代次护栏 + 读端点单飞（ADR-0009 Decision 5）、分页 offset 溢出。
- **14-01（P2）已修复**：刷新模型 URL 版本段判定收敛单实现（vN 供应商 404）。
- **02-01（P2）已修复**：成员终态失败零日志 → 四终态补 error 日志（同批 03-03 dispatch/relay 失败路径零日志）。
- **08-01（P2）已修复**：SSE log 事件无 seq → 捕获侧 per-span 单调分配，前端去重生效 + 前后端回归各一。
- **21-01（P2）已修复**：初始化保存竞态 → saveInitSettings 移入 init onSuccess + 回归测试。
- **10-01（P1）已修复**：insight day 分位尾桶覆盖写 → 同层帧直落 + 跨层细帧时长加权合并 + 未闭段实时同权并入（d8f7825）。
- **10-02（P1）已修复**：provider-model-rank 单侧过滤快照全量读 → 主体集合过滤（d8f7825）。

## Not yet specified

- 「P1 高危险发现」是否中途脱离清单模式插入修复（默认不改，图后统一排期）——由用户在票答覆盖时自行决定。
- 图后实施排期的组织方式（统一 backlog 文档？按严重度分批？按模块批量？）——终点之后的交付形态，图内不定。
- AGENTS.md 结构树刷新（漂移清单见 MODULES.md §1.4）与各票低危「口味级」条目的收敛决策——均归图后实施批次，图内不再开票。

## Out of scope

- 产品向工作：代际立项（adaptive thinking 等模型能力）、额度闸门恢复讨论、新厂商接入。
- 前端视觉/UX 评审（本图只审代码质量，不含设计走查）。
- 2026-09-08 及此前审计已整改项的复查（除非重构使其回归）。
