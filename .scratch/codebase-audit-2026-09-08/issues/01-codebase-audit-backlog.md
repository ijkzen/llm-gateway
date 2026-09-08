# 01: 代码库四维审计整改（遗留问题登记）

**What to build:** 2026-09-08 代码库四维审计（内存/性能/错误分支/SQL 慢查询）发现 25 项问题，全部未整改。本条目作为整批整改工作的遗留跟踪，明细（每项的场景、代码证据 file:line、修法建议）见同目录 `../FINDINGS.md`。修复时按 FINDINGS 编号逐项勾销，涉及数据库迁移的项注意新迁移从版本 24 起编（生产库 14/15 号段废弃不可复用）。

**Blocked by:** None (can start immediately)

**Status:** ready-for-human

**整改建议顺序**（未拍板，按性价比推荐先修 P1 项）:

- [ ] **E1 + E2**（proxy 错误分支）: OpenAI Compat/原生非流式读失败吞成 200 假成功；上游流式中断记 success:true 且直通不补 [DONE]。改动小，直接纠正指标口径与客户端假成功。
- [ ] **性能 P1 + 错误 E3 + E6**（cron worker 日志落库）: 逐条 autocommit INSERT 改攒批事务，同时缓解广播 Lagged 丢日志（Lagged 视为截断）、seq/计数先自增后插与双 2001 冲突。
- [ ] **E5**（SSE 先订阅后快照）: 调换两行顺序，堵住快照与订阅之间日志永久丢失窗口。
- [ ] **E4**（insert_run 失败即收尾）: 杜绝孤儿日志与永久 running。
- [ ] **SQL S1 + S2 + S4**（request 表索引）: ttft/tps 索引挪出条件分支、补 (provider_id, model_id, success, start_time) 与 (provider_id, success, start_time)。
- [ ] **E7**（usage_cache upsert 单语句）: ON CONFLICT 化 + 落库失败不静默。
- [ ] **内存 M1**（Responses 转换流式全缓冲后重放）: 改动量最大（沿用 OpenAiCompat live 分支模式），涉及流式用户体验与峰值内存，建议单独立项排期；与 protocol-conversion-audit 遗留 C2 同源。
- [ ] **内存 M2/M3**（日志事件 Arc 化与容量核算）、**性能 P2**（SSE 增量解析）、**P3**（用量内存缓存+单飞）: 中/低优先。
- [ ] 其余 P3 项与 SQL S3/S5/S6（request 表保留策略/rollup、insight 查询合并与分位采样、cron SQL 写法）: 视产品阶段决定。
