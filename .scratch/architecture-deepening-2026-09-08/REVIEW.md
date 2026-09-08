# 架构深化评审 — REVIEW（2026-09-08）

本次评审由 improve-codebase-architecture 流程产出：git 热点扫描（近月 src/proxy/mod.rs 41 次改动居首，usage/stats/providers 路由次之）+ 4 路子代理逐区摩擦调查 + 关键证据人工核验。全部决策经 grilling 逐轮拍板，已达成共享理解。

## 拍板决策总表

| # | 候选 | 拍板要点 | 工单 |
| --- | --- | --- | --- |
| C1 | 成员尝试循环合一 | 只收尝试循环（排序留在调用方）；dispatch_success 四臂与流式指标交错**不入本轮**（留作后续独立轮） | 01 |
| C2 | 可用性谓词单一来源 | availability 组合谓词（读侧）+ UsageData 判定方法原位在 usage/types；**选路相关 4 消费点全量切换**（order_members retain / forward_chat_direct 裸查 / probe_gate / apply_usage_gate） | 02 |
| C3 | 数据面板聚合核心 | 「今日/分桶时区」**统一到设置表时区**（与 6b77d96 口径合流）；Top-N+其他折叠**留客户端**（不碰 FE 契约）；request_logs 与 stats 边界语义统一 | 03/04/05 |
| C4 | 调度状态单一所有者 | 完整状态机唯一所有者：worker 执行完**回调调度器**（on_run_finished），路由/worker 不再直写任务表；仓库哑持久化 | 06 |
| C5 | 用量预估纯核心 | 抽纯函数并**固化 645bca1 现状语义**（estimatable = used>0 ∧ ratio，无按天闸门），信任边界写成显式谓词；闸门恢复与否是独立产品讨论不夹带 | 07 |

## 关键证据（工单引用锚点）

- **C1 活 bug**：`forward_chat` 对空候选返回 503（proxy/mod.rs ~1117-1131），`forward_native` 无守卫直接 `ordered[0]`（~1744-1749）→ panic。空候选真实可达：用量缓存 10 分钟内为 0 而额度门控（@every 5m）尚未停用。
- **C1 双循环**：forward_chat 1014-1405 vs forward_native 1643-1928，各自 ~10 处 record_failure + 降级/重试分支；仅错误整形（openai_error vs endpoint.error）不同。
- **C2 口径四写**：load_members 156-213（enable 过滤）、order_members retain 265-370（注释自称「同口径 as apply_usage_gate」，实为第三处）、usage_rank has_quota/worst_window 54-117、failure_recovery probe_gate 142-163；types.rs subscription_usable 179-200 为规范表述。
- **C3**：stats.rs 2405 行 12 端点全部手拼 `format!` SQL（WHERE 拼装 ×10、分桶补零 ×4、窗口契约 3 种解析）；五个 rank handler ≈ 同一参数化模块抄 5 遍（~600 行）；insight 单 handler 442 行；request_logs end_time 闭区间 vs stats 半开区间；parse_tz_offset 默认 0=UTC（stats.rs 47-51），「今日」在前端浏览器本地算（overview.tsx 48-55），调度/用量走设置表时区。percentile/ratio/补零零单测，仅 2490 行全栈集成测试兜底。
- **C4**：next_run_at 写者四处——路由 PUT 无条件重算（cron_jobs.rs ~146）、仓库 insert、调度器加载、worker 执行后（worker.rs 284-306，从 scheduled_at 锚定、超期从 now）；带回滚的 scheduler::set_enabled（339-404）仅测试用，生产走路由「update_job_full + update_job_in_memory」另一套。
- **C5**：get_provider_usage_estimate 140 行内联（routes/providers.rs 845-984）；三次修复 645bca1/d807c27/6b77d96（含一次修复自身回归）全住该区；信任边界只剩 965 行一行隐式判定；零单测。

## 本轮明确不做（避免重复建议）

- dispatch_success 四臂流式循环收拢与指标交错重构（proxy/mod.rs 2112-2546）——后续独立轮。
- Top-N+其他 折叠迁服务器侧——留客户端 utils topWithOther。
- 用量 fetcher 层「会话失效」分类分歧（CookieCloud 族 3xx=过期 vs 共享 helper 只认 401/403）与隐藏登录冷却状态——与「用量代码不抽重复」既有拍板相邻，未入本轮，观察。
- stats 与 request_logs 的关系未动（request entity 无查询 helper 的原始 SQL 现状随 03/05 收口逐步缓解，不引入查询框架）。

## 域词汇侧写（随工单实施时同步 CONTEXT.md）

- C2 实施时：可用性域补「选路可用 (Traffic-Eligible)」读侧术语。
- C7 实施时：用量域补「用量预估 (Usage Estimate)」术语。
