# FINDINGS · 08 cron 调度与日志链路审查（2026-09-10）

范围：`scheduler.rs`（757 行全读）/ `parser.rs`（366 行全读）/ `repository.rs`（380 行全读）/ `seed.rs`（182 行全读）/ `mod.rs` + worker.rs（1069 行）/ log_capture.rs（371 行）/ log_repository.rs（483 行）由后台子代理逐行深读盘点（关键断言已磁盘抽核：JobLogEvent seq 全 None、run_ended 先于落库、SSE 路由与前端去重实现亲验）+ `scheduler/tests.rs` 盘点 + lib.rs init/优雅关闭收尾段 + failure_recovery 归属审查。已知边界：09-08 已修 E1-E8/P1-P4 系列核验无回归（攒批 insert_many/E3 Lagged→note_lost/E5 seq 计数口径含双 2001 测试/E6 先订阅后快照/current_thread+SUBSCRIBER_LOCK/next_run_at 写者 C4 收口），不重记。清单模式：不改代码。**本票 1 P2（08-01 SSE seq 契约，已拍板修复方案）+ 12 P3 + 1 归属定夺（已拍板移顶层）**。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 08-01 | P2·契约【已修复 2026-09-10】 | 逻辑/跨栈契约 | 实时 SSE log 事件从不携带 seq（JobLogEvent.seq 全 None），而 E6「先订阅再快照」防重设计与前端去重（`data.seq <= last.seq`）都建立在 seq 上——重叠窗口日志重复追加且 React key 恒 undefined |
| 08-02 | P3 | 逻辑/时序 | run_ended 广播先于 finish_run 落库（worker.rs:263 vs 271）——订阅窗口落在两者间的 SSE 连接看到「running 永不结束」；交换顺序可消除 |
| 08-03 | P3 | 简洁/口径 | worker 侧失败日志与截断提示消息不经 4096 截断（:378/393/426），与 log_capture 捕获侧口径不一致——超长错误串落库超 4096 无约束 |
| 08-04 | P3 | 简洁/死面 | log_repository trait 的 `insert_log` 单行方法生产零调用（trait 80-87 / 实现 192-211），仅测试用——worker 走 inherent `insert_logs`，双路径并存 |
| 08-05 | P3 | 逻辑/记账 | flush 失败静默丢批（worker.rs:459-468）：pending 清空无重试，run.log_count/truncated 与真实行数漂移——注释背书，观察级 |
| 08-06 | P3 | 逻辑/状态机 | cron_job_runs.status 三写者（finish_run/启动恢复/6h 回收）：6h 回收会对仍在真实执行的超长 handler 标 failed，之后 finish_run 覆盖为 success（DB 中间态）——内置任务不受影响，注释自洽 |
| 08-07 | P3 | 逻辑/展示 | seed 行 next_run_at=now（seed.rs:123-124）非计算值——@hourly 的 failure_recovery 首轮展示「下次运行=启动时刻」直到首跑 |
| 08-08 | P3 | 测试覆盖 | 六类缺口：shutdown 超时后重启恢复、队列满、Lagged 真实溢出集成、并发双 run 日志归属隔离、insert_run 失败注入、run_ended/落库顺序竞态 |
| 08-09 | P3 | 测试覆盖 | log_repository 三缺口：并发 prune、finish_run 覆盖 6h 回收行、insert_many 大批次参数边界 |
| 08-10 | P3 | 简洁/微观察 | worker.rs:211-222 idle_flush sleep 每循环重建（pending 空也建）；update_run_times 不动 updated_at（列表 updated_at 恒为定义编辑时间，语义观察）；update_job_in_memory 重建窗口丢一次触发（remove→add 间隙，@every 重置间隔，重启同语义） |
| 归属 | — | 模块归属【已拍板：移顶层】 | failure_recovery.rs 寄居 proxy/ 名不副实：唯一消费=cron 注册（lib.rs:196-217），转发路径零调用；移 src/failure_recovery.rs 与 availability.rs 平级 |

## 各条证据

### 08-01 实时 SSE log 事件无 seq：E6 防重契约坏（P2，跨栈契约）【已修复 2026-09-10】

全链路核验（三层亲验）：

- **事件侧**：`JobLogEvent.seq: Option<i32>`（log_capture.rs:43，`skip_serializing_if`），生产发送点全部赋 None——log 事件 :151、run_started :63、run_ended :83。结构体文档（:31-33）声称「log：…携带 `seq`/`level`/`message`」——注释与实现不符。DB 侧 seq 只在 worker flush 时分配（worker.rs:441-456，落库即分配），广播事件从未携带。
- **路由侧**：routes/cron_jobs.rs:336-339 注释明示「先订阅再读库快照…（与快照重叠部分前端按 seq 去重）」——先订阅后快照是 E6 修复（防永久丢窗口），其防重依赖 seq；快照日志条目从 DB 行带 seq（:354-358），实时 log 事件原样序列化 JobLogEvent（:366）无 seq。
- **前端侧**：use-cron-job-logs.ts:134-135 `if (last && data.seq <= last.seq) return s;`——live 事件 `data.seq` 为 undefined，`undefined <= last.seq` 恒 false → **去重从未触发**；重叠窗口（执行中打开弹窗、事件已 flush 进快照且又在订阅后经广播送达）日志重复追加；CronJobLogsDialog.tsx:206 以 log.seq 作 React key 恒 undefined。

实际触发窗口窄（订阅到快照读之间的毫秒级 + 已 flush），但契约是坏的——E6 修复引入的防重机制整体空转，且注释、前端类型（`data.seq: number`）与实现三方不一致。

**拍板（2026-09-10）**：全链路补 seq。实施指引（入实施批）：
1. seq 分配点=捕获侧：`JobLogLayer` 在 on_event 确定 span 归属后，按 (job_name, run_id) 的 per-span 单调计数器赋 seq（broadcast FIFO 保序，worker 与 SSE 订阅到同一序号序列）；run_started/run_ended 保持 seq=None（前端不比较）。
2. 与 DB seq 的一致性论证：同 run 内捕获序 == worker consume 序 == flush 落库序（无 Lagged 时），layer 分配 seq 与 DB seq 同源同序；Lagged 丢事件后 worker note_lost 合成提示行只落 DB（不进 broadcast）使 DB 号段比 live 多——但去重规则「live seq ≤ snapshot 尾 seq 即丢」对该场景仍安全（重复事件 live seq ≤ 其 DB seq ≤ 快照尾），不错丢新事件；SSE 订阅者自身 Lagged 已有 reset 事件重拉兜底。
3. 前端保持 seq 去重不动（类型已对）；补一个「快照尾 seq 之后丢弃 ≤ 尾 seq 的 live 事件」的显式单测（前端 vitest，MockEventSource 驱动）。
4. log_capture 结构体文档（:31-33）与实现同步。

### 08-02 run_ended 广播先于 finish_run 落库（P3，时序）

worker.rs:263-269 广播 run_ended（携带 status/truncated），:271-282 才 finish_run 落库。SSE 订阅者若落在两者之间：初始快照读到 DB 仍 running（:339-352 分支），run_ended 已发出未被捕获 → 客户端停留在「running 永不结束」（无后续事件可收敛，只有重连/reset 能恢复）。反序（先落库后广播）的窗口是「快照读到 success → idle 分支」后收到 run_ended——idle 已清空本地状态，run_ended 由前端忽略或无害（需前端确认 idle 分支对迟到 run_ended 的处理）。默认解：交换顺序（finish_run 先行，成功后再广播 run_ended），并补一个顺序回归测试（08-08 缺口之一）。

### 08-03 worker 合成消息不经 4096 截断（P3，口径）

log_capture.rs:247-256 捕获侧消息统一 `trim_and_limit`（4096）；worker 侧三条合成消息直 push 不截断：截断提示（worker.rs:378）、Lagged 丢失提示（:426）、失败系统日志（:393）。handler 失败错误串（JobError Display，可含上游响应片段）超长时 DB 行超 4096 字符惯例无约束（entity 无长度限制）。默认解：三处经同一 trim（sink 内已有截断工具，复用）。

### 08-04 log_repository trait insert_log 死面（P3，简洁）

trait `CronJobLogRepository::insert_log` 单行（log_repository.rs:80-87）+ SeaOrm 实现（:192-211）生产零调用——worker 走 inherent `insert_logs`（批量多值 INSERT）；单行方法仅本文件测试使用（:349-366）。接口面与生产路径不一致。默认解：删 trait 方法与实现（测试改用 insert_logs），或 worker 侧经 trait 消除双路径——倾向删除（trait 保留其余读侧方法）。

### 08-05 flush 失败静默丢批（P3，记账边界，观察级）

worker.rs:459-468：flush（insert_many）失败 → pending 清空、无重试、warn 一次。DB 故障下该 run 的 log_count/truncated 与真实行数漂移（log_count 在 flush 成功才推进，失败批不计入——DB 行少、计数少，自洽；但 truncated 语义与「丢失」提示缺失）。E7 教训（落库失败不静默）在此是 warn + 丢批，无退避重试。内置任务周期短、下轮自愈；DB 故障本身使其他写路径同样失败。观察级，默认解=flush 失败保留 pending 至下轮（或单次重试），随实施批评估。

### 08-06 cron_job_runs.status 三写者与 6h 回收中间态（P3，状态机）

写者：worker finish_run（worker.rs:272）、启动 mark_interrupted_runs_failed（lib.rs:126-133）、prune 内 6h 超时回收（log_repository.rs:261-271）。6h 回收对「仍在真实执行」的超长 handler（无心跳机制，仅靠 started_at 阈值）标 failed；该 run 若之后正常结束，finish_run 只按 run_id 更新不校验原状态 → DB 中间态 failed→success（ended_at 曾被提前置值再覆盖）。内置任务（≤5min）不触发；自定义超长 handler 才有。注释自洽（log_repository.rs:258-260），默认解=文档化该风险（entity/AGENTS 注记）或给 finish_run 加「仅 running 态可终态化」守卫（需评估 6h 回收与 finish 并发的取舍），随实施批。

### 08-07 seed 行 next_run_at=now（P3，展示）

seed.rs:123-124 种子行 last_run_at=now 且 next_run_at=now（非 compute_next_run 计算值）。@every 任务由 load_from_db 的 reset_every_schedule（scheduler.rs:653-682）重算 ✓；@hourly 的 failure_recovery 的 next_run_at 保持「启动时刻」直到首跑完成——列表/详情展示「下次运行=过去时刻」数小时。默认解：ensure_job 用 compute_next_run_tz(expression) 计算 next_run_at（失败兜底 now）；一行改动 + seed 测试补断言。

### 08-08 worker 执行链测试缺口六类（P3，测试覆盖）

worker.rs 14 例单测覆盖执行/回写/失败/panic/超时/截断/丢批/清理/禁用语义，缺口：
① shutdown 超时放弃后「run 留 running + 下次启动 mark_interrupted 恢复」链路无测试（lib.rs:126 与 worker shutdown 的衔接）；② 队列满时 worker_tx send 阻塞语义与 Closed 错误路径（scheduler 侧仅 drop-rx 一种）；③ Lagged 真实 broadcast 溢出集成测试（现仅 note_lost 直驱单测）；④ panic handler + 捕获 subscriber 下失败日志落库（:613 panic 测试不注册捕获层）；⑤ 同一 job 并发双 run 的日志归属隔离（按 run_id 过滤）；⑥ insert_run 失败分支注入式测试（现仅 sink 禁用模拟）。

### 08-09 log_repository 测试缺口（P3，测试覆盖）

log_repository.rs 8 例单测（insert/finish/list/prune×2/启动恢复/孤儿清理/6h 回收/serializable），缺口：并发两个 prune 互不干扰；finish_run 对「已被 6h 回收标 failed」行的覆盖行为（08-06 的测试锚）；insert_many 大批次（>50 行）参数边界。

### 08-10 微观察三则（P3，简洁/语义）

① worker.rs:211-222 idle_flush sleep 每 select 循环重建（pending 空也建 sleep，微浪费，非缺陷）；② repository update_run_times 不动 updated_at（cron 列表 updated_at 恒为定义编辑时间，运行不刷新——语义观察，前端若按 updated_at 排序需知悉）；③ update_job_in_memory 表达式/enabled 变更路径 remove→add 重建（scheduler.rs:452-471）：间隙内 scheduler tick 不触发（丢一次），@every 重置间隔——与重启语义一致，接受。

## 归属审查：failure_recovery.rs 寄居定夺【已拍板：移顶层】

- **现状**：src/proxy/failure_recovery.rs（271 行）内容=failure 停用供应商恢复编排（查库 → probe_gate 用量判定 → test_model 探测 → availability::recover_probe 乐观锁恢复）。唯一消费=lib.rs:196-217 FAILURE_RECOVERY_JOB handler 注册 + tests/provider_failure_recovery_integration.rs（import `llm_gateway::proxy::failure_recovery::recover_failure_disabled`）；proxy 转发路径零调用（04 票已核）。
- **对照惯例**：usage_refresh handler 业务体住业务域（usage/persist.rs::refresh_all_usage）；failure_recovery 无自然业务宿主（availability.rs 是低层底座，反向依赖 usage/proxy 会破坏分层）。
- **拍板（2026-09-10）**：移顶层 `src/failure_recovery.rs`（与 availability.rs 平级，语义=失败恢复业务域）。依赖面核对：entity/crypto/state/usage::persist::fetch_and_store/availability/proxy::test_model（proxy/mod.rs:41 `pub use probe::{…, test_model}` 已 pub，顶层模块可访问，无环——proxy 不反向依赖它）。实施批：移动文件 + lib.rs:196 引用路径改 `crate::failure_recovery::recover_failure_disabled` + tests/provider_failure_recovery_integration.rs import 路径 + AGENTS.md 结构树（proxy/ 目录与顶层文件清单）+ failure_recovery.rs 内 `super::test_model` → `crate::proxy::test_model`。零行为变化。

## 已核验无问题区（避免后续票重复审查）

- **09-08 已修系列无回归**：P1 攒批 insert_many（worker.rs:437-469 → log_repository.rs:125-145，flush 成功才推进 seq/log_count）；E3 Lagged→note_lost 置 truncated+追加丢失提示（worker.rs:414-433）；E5 seq/计数口径（≤2000 真实 + 至多 1 截断提示 counts=false + 1 失败日志 counts=true，共用同一 seq 通道，双 2001 回归测试 worker.rs:897-956 锁定）；E6 先订阅后快照（routes:337-339）——本身无回归，其防重依赖的 seq 契约缺陷见 08-01。
- **禁用=移除语义**：set_enabled 禁用走 scheduler.remove（scheduler.rs:382-402），启用重建；disabled job 留内存 map 可手动执行（add_job_internal :592-599 注释 + 测试 :647/:720 锁定）；修改锁 modification_lock 串行化并发变更（测试 :411）。
- **回滚路径完备**：add_job DB 失败回滚 scheduler（scheduler.rs:148-158）、set_enabled 失败回滚 DB（:369-381/388-396）、update_job_in_memory 重建失败恢复原 job（:454-471）、remove_job_from_scheduler 失败恢复 map（:621-644）——FailingRepo 宏三件套测试锁定（scheduler/tests.rs:284/323/364）。
- **写者路径收敛**：cron_jobs.last/next_run_at 运行时唯一写者=worker 经 scheduler::on_run_finished（worker.rs:291 → scheduler.rs:728-754，scheduled_at 锚定 + 过期重算，C4 收口无回归）；日志落库唯一写者=worker 批写；run 状态三写者收敛语义见 08-06。
- **启动恢复与清理**：mark_interrupted_runs_failed + delete_orphan_logs（log_repository.rs:232-255）由 lib.rs:126-133 启动调用；prune 事务首语句先裸查再写删（:286-289 注释，517 教训同款规避已落地）；30 次边界 keep+1 阈值测试锁定（worker.rs:1006-1068）。
- **优雅关闭**：停调度器（不再派发）→ worker shutdown（abort 派发循环 + acquire_many 超时等在跑任务）→ 超时后进程退出留 running 由下次启动恢复（lib.rs:285-293，文档口径一致）。
- **seed 与 handler 双源一致**：lib.rs:243-250 seed 四任务与 :158-243 handler 注册一一对应、顺序在 load_from_db 之前（种子当轮进调度器）；seed 幂等单测锁定。
- **捕获层**：JobLogLayer 只登记 target=cron_job_log 且带 job_name+run_id 的 span（log_capture.rs:118-130），event_scope 由内向外归属（:166-181，嵌套/contextual 测试 :300-337）；Arc 共享事件体（M2）；每事件持锁查 map 短临界区。
- **删/禁用后 in-flight 任务**：照常执行并记录（无取消机制），on_run_finished 因 is_deleted 过滤返回 false→warn（repository.rs:143-153），残留由本次 prune 收敛——无泄漏。

## 性能/内存轮结论

无 P1/P2。正向：Arc<JobLogEvent> 广播避免逐订阅者深克隆（M2）；日志攒批 insert_many（P1）；有界队列 + 信号量并发池背压；prune 事务批删。P3 级：08-05（flush 失败丢批无重试——DB 故障期日志丢失面，观察级）、08-10-①（idle_flush sleep 重建微浪费）。结论：执行与日志链路形态适合当前规模（单用户网关、内置任务 ≤5 个、单次 ≤2000 条日志），无需结构性改动。

## 实施进度

- **08-01 已修复**：`log_capture.rs::JobLogLayer` 的 span 登记表由 `(job_name, run_id)` 扩为 `(job_name, run_id, next_seq)`，`on_event` 在同一把锁内自增分配 per-span 单调 `seq` 并写入 log 事件（run_started/run_ended 仍为 None）；`owning_span_next_seq` 替代原 `lookup_owner`。广播 FIFO 保序 ⇒ SSE seq 与 worker 落库 seq 同源，前端既有 `data.seq <= last.seq` 去重生效。单测：`test_captures_events_inside_job_span` 补 seq 断言、新增 `test_seq_is_per_span_monotonic_and_restarts_per_run`（每次执行独立从 1 起编）；前端新增 `cron-job-logs-dialog` 去重回归（快照 1/2 + 重叠实时事件各只渲染一条、seq 更大的新事件正常追加）。
