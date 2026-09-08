# FINDINGS：代码库四维审计（内存 / 性能 / 错误分支 / SQL 慢查询）

- **日期**: 2026-09-08
- **范围**: `src/proxy/`（转发管线、上游客户端、连接池、SSE 工具、指标落库）、`src/cron/`（日志捕获 → broadcast → worker → 日志仓库）、`src/routes/stats.rs` / `request_logs.rs` / `providers.rs`（聚合查询）、`src/usage/persist.rs`（用量缓存）、`src/db.rs`（索引迁移）、`src/entity/`
- **方法**: 主代理通读 proxy 全链路与 usage/persist；两个子代理分别深读 cron 日志链路与 SQL/索引侧；所有引用行号经主代理逐条抽查复核（与磁盘现状一致）
- **状态**: 全部未整改。整改工作整体作为遗留问题登记，见 `issues/01-codebase-audit-backlog.md`
- **严重度**: [P1] 高（正确性/吞吐/用户可见）[P2] 中 [P3] 低

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| M1 | P1 | 内存 | Responses 转换流式全量缓冲后重放：TTFB=全量生成耗时，峰值内存=整条输出 JSON 树 |
| M2 | P2 | 内存 | cron 日志事件 ~4 处堆分配 + broadcast 每订阅者克隆放大 |
| M3 | P2 | 内存 | 8192 槽广播最坏驻留数十 MB，容量与并发/单 run 上限不挂钩 |
| M4 | P3 | 内存 | 日志 ts RFC3339 白算：落库时重新取时间，SSE 与 DB 时间漂移 |
| M5 | P3 | 内存 | SQLite 页缓存按连接放大（256MB × 5 连接） |
| P1 | P1 | 性能 | cron 日志逐条 autocommit INSERT，单 run ≤2000 次 DB 往返 |
| P2 | P2 | 性能 | 流式每 chunk 4-5 次整段拷贝；SseSplitter drain 每行 O(n) memmove |
| P3 | P2 | 性能 | /v1 热路径每请求 N 次串行 DB 读；缓存过期即每请求真实抓厂商 API（无单飞） |
| P4 | P3 | 性能 | request 指标每行 spawn 一个 INSERT 任务，failover 每尝试额外一行 |
| P5 | P3 | 性能 | 用量抓取每 5 分钟新建 26 个 reqwest 客户端，全部丢弃连接池/TLS 会话 |
| P6 | P3 | 性能 | rustls ClientConfig 每次新连接 clone |
| E1 | P1 | 错误分支 | OpenAI Compat 非流式读上游失败吞成 200 `{}` 假成功 + success:true |
| E2 | P1 | 错误分支 | 上游流式中断被记成功；OpenAI 直通不补发 [DONE] |
| E3 | P1 | 错误分支 | 广播 Lagged 静默丢日志不置截断标记，run 呈现无提示 seq 空洞 |
| E4 | P1 | 错误分支 | insert_run 失败仍执行 → 孤儿日志永不被清理；finish_run 失败 run 永久 running |
| E5 | P2 | 错误分支 | SSE 先读 DB 快照后 subscribe，中间窗口日志对该连接永久丢失 |
| E6 | P2 | 错误分支 | 日志计数先自增后插库；截断提示与失败日志 seq 冲突（双 2001） |
| E7 | P2 | 错误分支 | usage_cache 两段式读写非原子；落库失败被吞、无退避反复抓取 |
| E8 | P3 | 错误分支 | run_job_now 队列满时无限阻塞 HTTP 请求 |
| S1 | P1 | SQL | 新库缺 idx_request_ttft/idx_request_tps（索引建在老库条件分支内） |
| S2 | P1 | SQL | request.model_id 无索引，却是图表/排行/过滤的一级维度 |
| S3 | P1 | SQL | request 表只增不减；summary 默认全历史聚合 O(N) 无上限 |
| S4 | P2 | SQL | provider 单点过滤缺 provider 前缀复合索引 |
| S5 | P2 | SQL | insight 一次窗口 ~13 次独立聚合扫描 + 分位全量逐值拉回 |
| S6 | P3 | SQL | cron 日志 prune 全量 SELECT 无 LIMIT；(run_id) 索引不含 seq |

## 一、内存浪费

### M1 [P1] Responses 转换流式「全量缓冲后重放」

- **场景**: Chat 请求（`/v1/chat/completions`）路由到 Responses 协议成员时，即使客户端 `stream=true`，网关也先把上游整条流逐事件 parse 成 `Vec<Value>` 缓冲到 EOF，才 spawn 任务逐块回放。后果：① 客户端 TTFB = 上游全量生成耗时，流式对客户端退化为非流式（首字延迟拉满，长思考/长输出体验差）；② 峰值内存 = 整条输出 JSON Value 树，十万 token 级思考流可达数十 MB；③ 缓冲期间客户端断开、上游中断均感知不到，直到收流完毕。原生 `/v1/responses` 透传分支反而是 live 旁路中继，唯独转换路径缓冲。
- **证据**:
  - `src/proxy/mod.rs:2231-2240` — `(Protocol::OpenAiResponses, _)` 分支先 `collect_stream_events` 收完整条流
  - `src/proxy/mod.rs:2286-2315` — 收流结束后才 spawn 任务把 `events.chunks` 回放给客户端
  - `src/proxy/mod.rs:2557-2565` — `collect_stream_events` 把 chunk 全部压进 `Vec<Value>`
  - 对照：OpenAiCompat 流式 live 中继 `src/proxy/mod.rs:2168-2228`；原生 Responses 透传 live 扫描 `src/proxy/mod.rs:2060-2100`
- **备注**: 与 `.scratch/protocol-conversion-audit` FINDINGS 遗留项「C2 流式缓冲」同源，仍未修。
- **修法**: 流式客户端沿用 OpenAiCompat 直通分支模式——逐事件 convert 后立即 send（含 reasoning_exclude 剥除、usage 尾块、[DONE]）；仅 `stream=false` 才走 `accumulate_chunks`。

### M2 [P2] cron 日志事件多次堆分配 + broadcast 每订阅者克隆放大

- **场景**: handler 每次打日志：`lookup_owner` 沿 span 链查归属时每事件 clone 一次 `job_name`/`run_id` 字符串（:173）；`on_event` 再为 `level.to_string()`（:148）、`ts.to_rfc3339()`（:152）分配；broadcast 的 `recv()` 对**每个**接收者各 clone 一遍整条事件（1 个 worker 消费者 + 每个 SSE 连接）；多个并发 run 时各 run 消费者还会把别 run 的事件 clone 后丢弃（见 P1 旧条目 P3）。
- **证据**: `src/cron/log_capture.rs:132-154`（on_event 构造 JobLogEvent）；`src/cron/log_capture.rs:162-177`（lookup_owner 循环内逐层上锁 + `owner.clone()`）
- **修法**: 事件体改 `Arc<JobLogEvent>` 单次分配，或按 job 拆分子 channel。

### M3 [P2] 8192 槽广播通道最坏驻留数十 MB

- **场景**: 所有任务日志共享一个容量 8192 的 broadcast ring；每条消息带 job_name/run_id/level/message/ts 且消息长度有 4096 字符上限。满长消息 × 满槽时瞬时驻留可达数十 MB（叠加 M2 的每订阅者克隆）。ring 覆盖式写入不会无限增长，但容量与「并发 run 数 × 单 run 日志上限」不挂钩；真正的风险是溢出即丢数据（见 E3）。
- **证据**: `src/lib.rs`（通道容量 8192 注释）；`src/cron/log_capture.rs:140`（`trim_and_limit` 消息裁剪入口）
- **修法**: 维持有界即可；容量按并发 run × 单 run 上限核算并注释依据。

### M4 [P3] 日志 ts 字符串白算，SSE 与 DB 时间漂移

- **场景**: 每条事件捕获时算一次 `Utc::now().to_rfc3339()` 堆字符串（只服务 SSE 推送），落库路径却用 `Utc::now()` 重新取时间插库——浪费一次分配，且同一行日志 SSE 时间与 DB 时间不一致（worker 消费滞后越大偏差越大）。
- **证据**: `src/cron/log_capture.rs:152`（ts 计算）；`src/cron/worker.rs:336`（落库重新取时间）
- **修法**: 落库复用 `event.ts`，或将 ts 改为 i64 毫秒。

### M5 [P3] SQLite 页缓存按连接放大

- **场景**: `cache_size=-64000`（约 256MB/连接）与 `mmap_size=256MB` 在 SQLite 中是逐连接生效的；连接池 `max_connections(5)` 全部活跃时页缓存理论最坏 ~1.3GB/进程。
- **证据**: `src/db.rs:82`（cache_size）、`src/db.rs:90`（mmap_size）、`src/db.rs:60`（max_connections 5）
- **修法**: 属配置认知项；如进程内存受限可按连接调小或换 SQLite 共享缓存。

## 二、换种写法性能更好

### P1 [P1] cron 日志逐条 autocommit INSERT（吞吐瓶颈）

- **场景**: worker 消费循环对每条日志一次独立 `insert_log`（单行 ActiveModel insert，autocommit 事务）；单 run 最多 2000 条、10 个 run 并发共享 max 5 连接池。日志产生快于落库时广播 ring 先被填满 → E3 丢日志。DB 写是整条管线实际限速环节。
- **证据**: `src/cron/worker.rs:333-341`（persist_log_event 逐条 insert）；`src/cron/log_repository.rs:158-177`（insert_log 单行插入）
- **修法**: 消费者按 run 攒批（每 ~50 条或 50ms 一个事务 `insert_many`），run 结束统一 flush。

### P2 [P2] 流式每 chunk 4-5 次整段拷贝 + SseSplitter 每行 O(n) memmove

- **场景**: 流式直通/转换路径每个上游数据帧：`bytes` → `from_utf8_lossy(...).to_string()` 整段拷贝 → splitter buffer 再追加 → 逐行 `line.to_string()` → `data_lines.join()` → `sse_frame(format!)` → `Bytes`。另 `SseSplitter::feed` 用 `buffer.drain(..pos+1)` 逐行从 String 头部删，单帧内多事件时每行 O(n) memmove（近似 O(n²)）。高吞吐长流（上万事件）持续分配/拷贝抖动。
- **证据**: `src/proxy/sse.rs:15-42`（feed：push_str + 逐行 to_string + drain(..pos+1)）；`src/proxy/sse.rs:45-47`（sse_frame format!）；`src/proxy/mod.rs:2186-2200`（直通分支每帧 lossy 拷贝）、`:2448-2459`（转换分支同款）
- **修法**: 对字节缓冲做增量解析（只查边界不整段拷贝）+ 复用一块写出缓冲逐帧 format。

### P3 [P2] /v1 热路径每请求 N 次串行 DB 读；缓存过期即每请求真实抓厂商 API（无单飞）

- **场景**: 策略 0/1 下每次转发（LB 排序）都走 `resolve_usage_map`：对虚拟模型去重后的每个 provider **串行** `read_usage_cache`（一次 DB 往返/个）；缓存缺失/过期时该请求直接 `JoinSet` 并发真实抓取外部厂商用量 API（`fetch_and_store`）——冷缓存/刚过 TTL 时，聊天请求被厂商用量接口拖慢数百 ms 起，且并发请求会重复抓同一厂商（无 single-flight）。AppState 无任何内存用量缓存，「10 分钟数据库缓存」只省了抓取、没省读。
- **证据**: `src/proxy/mod.rs:436-472`（resolve_usage_map 串行读 + JoinSet 抓取）；`src/usage/persist.rs:29-44`（read_usage_cache）；`src/state.rs:12-28`（AppState 无用量内存缓存字段）
- **修法**: 一次 `WHERE provider_id IN (...)` 查询 + AppState 内 10 分钟 TTL 内存缓存（参照 `state.settings` 热缓存先例）+ 内存层单飞去重。

### P4 [P3] request 指标每行 spawn 一个 INSERT 任务

- **场景**: 每次转发完成/失败 spawn 一个独立任务执行单行 INSERT；failover 每个降级尝试还额外插一行（request_id 带 `-N` 后缀）。高 RPS + 上游批量失败时写入放大到 1+N 行 × 单独事务 × spawn，5 连接池排队。
- **证据**: `src/proxy/metrics.rs:162-168`（insert 内 tokio::spawn）；`src/proxy/mod.rs:1359-1371`（record_degraded 每降级一步一行）
- **修法**: 单一批量写任务，攒 10-50 行一个事务提交。

### P5 [P3] 用量抓取每轮新建 reqwest 客户端

- **场景**: `usage_refresh` 每 5 分钟刷新全部用量供应商时，每家独立构造 reqwest 客户端（注释「每家用独立 reqwest 客户端」），reqwest 连接池/TLS 会话随客户端 drop 全丢——每轮全部厂商重新 TCP+TLS 握手；多跳厂商（登录→查询）同轮内也不复用。
- **证据**: `src/usage/persist.rs:118-129`（refresh_all_usage 每家 spawn）；`src/usage/http.rs:37-56`（with_proxy 每调用新建 Client）
- **修法**: 客户端按 proxy 维度（直连/代理地址）复用，挂在 AppState。

### P6 [P3] rustls ClientConfig 每次新连接 clone

- **场景**: `tls_config()` 用 OnceLock 缓存配置，但每次新连接 `tls_config().clone()`——rustls ClientConfig 的 Clone 不是纯 Arc 浅拷贝，属不必要开销（连接池 miss 时每新连接一次）。
- **证据**: `src/proxy/upstream.rs:116-126`（tls_config OnceLock）、`src/proxy/upstream.rs:209`（每次 clone）
- **修法**: 包一层 `Arc<ClientConfig>` 后克隆。

## 三、错误/失败分支处理不完整

### E1 [P1] OpenAI Compat 非流式读上游失败吞成「200 假成功」

- **场景**: 上游返回 200 后响应体读取失败/超时（NON_STREAM_BODY_TIMEOUT=120s），`read_body(...).unwrap_or_default()` 拿空体 → parse 失败落 `json!({})` → 客户端收到 200 `{}` 空 completion，request 表记 `success: true`。原生非流式路径（:2025）同款。Anthropic/Gemini 非流式（:2346-2367）与 Responses 转换（:2241-2255）都有完整的 502 + `record_failure`，唯独这两条直通路径失败分支缺失——同函数内行为不对称。
- **证据**: `src/proxy/mod.rs:2138-2144`（OpenAI Compat 非流式：unwrap_or_default → 200 Json）；`src/proxy/mod.rs:2025`（原生非流式同款）
- **修法**: 读失败/解析失败统一走 `record_failure` + 502（复用 Anthropic 分支写法）。

### E2 [P1] 上游流式中断被记成功；OpenAI 直通不补发 [DONE]

- **场景**: 上游 hyper 帧错误（连接被对端重置等）时：OpenAI 直通分支（:2181-2184）与 Anthropic/Gemini 转换分支（:2441-2446）都是向客户端 `tx.send(Err(...))` 后 break——客户端 SSE 流以裸错误终止；OpenAI 直通分支**不补发 `[DONE]`/error 帧**（openai SDK 会报 truncated stream），转换分支只在 `converter.error()` 有值时给 error 帧。两条路径的 `RequestRecord` 均仍 `success: true` 且 fail_reason 为空——只有「客户端提前断开」被标记（disconnect），上游侧断流既不记失败也不留原因，指标口径失真。
- **证据**: `src/proxy/mod.rs:2178-2226`（直通：frame Err 分支 break 后无失败标记、无 [DONE] 补发）；`src/proxy/mod.rs:2440-2543`（转换：frame Err 同款；:2521 `success = converter.error().is_none()` 不含 hyper 层错误）
- **修法**: 区分上游断流与客户端断开：hyper 错误 → 记 `success: false` + fail_reason，直通分支补发 error 帧与 [DONE]。

### E3 [P1] 广播 Lagged 静默丢日志、不置截断标记

- **场景**: 日志产生快于落库（见性能 P1）→ worker 消费者 `Lagged(n)` 仅 warn（主循环 :208-210；drain 循环 :236 直接静默）。被丢行不落库、run 的 `truncated` 不置位（它只由 MAX_LOG_PER_RUN 触发）→ 该 run 日志 seq 无提示跳号（如 1..500、1501..2000），列表 API/SSE 快照呈现残缺日志而 run 显示「未截断」；SSE 侧 reset 重拉 DB 也补不回（DB 里就没有）。
- **证据**: `src/cron/worker.rs:208-210`（Lagged 仅 warn）、`src/cron/worker.rs:236`（drain Lagged 静默）
- **修法**: Lagged 计为截断——置 `truncated` 并追加一条「N 条日志因缓冲溢出丢失」系统日志（与截断提示同路径）。

### E4 [P1] run 建记录失败仍继续执行，孤儿日志永不被清理

- **场景**: `insert_run` 失败仅 warn（:170-172）后照常执行 handler，随后每条 `insert_log` 写入 `cron_job_runs` 中不存在的 run_id——两表无外键，prune 从 run 表倒推删不到孤儿行（log_repository.rs:211-243），孤儿数据永久残留。`finish_run` 失败/rows_affected=0 同样仅 warn（worker.rs:273-278），run 永久停在 `running`——SSE 快照把它当「执行中」反复回放，只有进程重启的 `mark_interrupted_runs_failed`（log_repository.rs:198-209）兜底。
- **证据**: `src/cron/worker.rs:170-172`；`src/cron/worker.rs:273-281`；`src/cron/log_repository.rs:211-243`（prune 从 run 倒推）
- **修法**: insert_run 失败即按 failed 收尾并跳过日志落库（或清理已写孤儿行）；finish/prune 失败加定期回收而非仅重启兜底。

### E5 [P2] SSE 快照先读库后 subscribe，中间窗口日志永久丢失

- **场景**: `stream_job_logs` 先 `list_runs`/`list_logs` 拼 snapshot（:335-356），随后才 `log_tx.subscribe()`（:358）。subscribe 不重放历史；若某日志事件已进广播 ring、尚未落库、又早于 subscribe 时刻——该连接既不在 DB 快照里、也收不到实时流，且不触发 reset（非 Lagged），形成跨快照/实时的静默空洞。
- **证据**: `src/routes/cron_jobs.rs:335-356`（DB 快照）vs `src/routes/cron_jobs.rs:358`（subscribe）
- **修法**: 调换顺序：先 subscribe 再读 DB 快照（先到事件与快照重叠部分靠前端 seq 去重）。

### E6 [P2] 日志计数先自增后插库；截断提示与失败日志 seq 冲突

- **场景**: `persist_log_event` 里 `*seq += 1; *log_count += 1` 先执行、insert 失败仅 warn（:333-340）→ seq 空洞 + run.log_count 虚高（列表条数与实际不符）。另截断提示以 `*seq + 1` 落库但**不回写 seq**（:312-331），若该 run 随后失败，失败日志 `seq += 1` 后也写 2001（:253-254）——同一 run 出现两行 `seq=2001`，按 seq 排序时次序不定。
- **证据**: `src/cron/worker.rs:253-254`（失败日志 seq 自增）、`src/cron/worker.rs:312-331`（截断提示不回写 seq）、`src/cron/worker.rs:333-340`（先自增后插）
- **修法**: insert 成功后才自增；截断提示落库后同步 `*seq += 1`；失败日志与截断提示用同一序号通道。

### E7 [P2] usage_cache 两段式读写非原子 + 落库失败被吞

- **场景**: `write_usage_cache` 先 `find` 再 update/insert（两段往返）——cron `usage_refresh` 与 `GET ?refresh=1` 并发刷新同一 provider 时双双读 None → 双 insert 撞唯一键失败。`fetch_and_store` 落库失败仅 warn 后照常返回成功数据：DB 持续故障时每轮（5 分钟或每请求过期）对全部厂商无退避地反复真实抓取，且缓存永远不新鲜。
- **证据**: `src/usage/persist.rs:47-76`（find → update/insert 两段式）；`src/usage/persist.rs:91-101`（fetch_and_store 吞落库错误）
- **修法**: `INSERT ... ON CONFLICT(provider_id) DO UPDATE` 单语句 upsert（provider_id 有唯一索引）；落库失败记录为抓取失败而非静默降级。

### E8 [P3] run_job_now 队列满时无限阻塞 HTTP 请求

- **场景**: 「立即执行」接口 `worker_tx.send(invocation).await` 直接把回压传到路由层，无超时。队列（容量 1000）被慢任务占满时 HTTP 请求可悬挂任意久，客户端得不到任何提示。
- **证据**: `src/cron/scheduler.rs:175-178`（send().await 无超时）
- **修法**: 带超时 send 或先 `try_reserve`，失败返回明确错误。

## 四、SQL 查询慢

### S1 [P1] 新库缺 idx_request_ttft / idx_request_tps（索引分叉）

- **场景**: `idx_request_ttft`、`idx_request_tps` 两条 CREATE INDEX 位于迁移 10 的 `network_latency_exists` 分支内（该分支只为老库 DROP COLUMN 兜底）；新装库走 else 只记版本号（:295-297），迁移 12（:319-328）也不补。而 `request_logs` 的排序白名单暴露按 `ttft`/`tps` 排序入口——**老库走索引，新库退化为整窗 temp sort**，同一代码两套行为。
- **证据**: `src/db.rs:281-298`（索引建在条件分支内）；`src/routes/request_logs.rs:28-37`（SORTABLE_COLUMNS 含 ttft/tps）
- **修法**: 把两条 CREATE INDEX 挪到无条件执行的后续迁移版本（现有迁移已编到 23，新增从 24 起；生产库 14/15 号段废弃不可复用）。

### S2 [P1] request.model_id 完全无索引，却是聚合/过滤一级维度

- **场景**: 图表模型分布 `GROUP BY p.name, r.model_id`（含 LEFT JOIN provider）与按 model_id 过滤、model_metrics 按 `provider_id + model_id` 点查、provider_model_rank 按 `provider_id, model_id` 分组、request_logs 的 `model_id IN (...)` 过滤——全部要先按时间窗范围扫窗口内所有行再做行级过滤 + temp B-tree 分组。
- **证据**: `src/routes/stats.rs:506-509`（model_id 过滤）、`src/routes/stats.rs:524-530`（模型分布 GROUP BY）、`src/routes/stats.rs:1529` 起（provider_model_rank，GROUP BY 在 :1553）、`src/routes/stats.rs:1847` 起（model_metrics，WHERE 在 :1868）、`src/routes/request_logs.rs:159-167`（model_id IN）
- **修法**: 建 `(provider_id, model_id, success, start_time)` 复合索引，一次覆盖 model_metrics / provider_model_rank / charts 分布与 usage 口径。

### S3 [P1] request 表只增不减 + summary 默认全历史聚合

- **场景**: 每次转发一行（metrics.rs:162-168）+ failover 每个降级尝试额外一行（mod.rs:1359-1371），全库无任何 request 表保留/清理策略；`summary` 端点不带时间窗时对全表 `COUNT(*)/SUM(...)`（overview 页每次打开默认全量）——O(N) 随流量线性上涨，索引无法消除全表 COUNT。
- **证据**: `src/routes/stats.rs:391-411`（无窗口 base_sql 无 WHERE）；`src/proxy/metrics.rs:162-168`；`src/proxy/mod.rs:1359-1371`
- **修法**: 加保留期定期清理（参照 cron 日志 30 次保留思路），或建小时/日 rollup 表、summary 改读 rollup。

### S4 [P2] provider 单点过滤缺 provider 前缀复合索引

- **场景**: `usage_estimate`（provider 粒度时间窗聚合）、provider_rank、provider_metrics 等只提供 `(start_time, provider_id, success)`——start_time 前缀，SQLite 只能先范围扫整个时间窗的索引项再行级过滤 provider；provider 少而窗口大（月/年）时浪费大。
- **证据**: `src/routes/providers.rs:925-929`（usage_estimate SQL WHERE provider_id + start_time 区间）；`src/routes/stats.rs:1389` 起（provider_rank，GROUP BY 在 :1405）；`src/routes/stats.rs:2038` 起（provider_metrics）
- **修法**: 补 `(provider_id, success, start_time)`（provider_id 点查截取时间区间）。

### S5 [P2] insight 一次窗口 ~13 次独立聚合扫描 + 分位全量逐值拉回

- **场景**: insight 端点对同一时间窗发起约 13 个独立聚合查询（:727-758 五个、:761-788 四个、:792-811 失败原因、:814-834 分位两个），各自扫一遍窗口；延迟分位把窗口内全部成功行的 `ttft`/`request_time` 逐值拉回 Rust 内存排序算 p50-p99——10 万成功请求即 10 万行 ×2。另月/年粒度时这些全窗扫描已跑完才在 `group_percentiles` 判 month_mode 丢弃（:1043-1046），应短路在发查询之前。窗口内逐行扫描重复 ~13 次，IO 放大 10 倍以上。
- **证据**: `src/routes/stats.rs:727-758`、`:761-788`、`:792-811`、`:814-834`（逐值分位查询）、`:1043-1046`（month_mode 后置丢弃）
- **修法**: 合并为单条 `GROUP BY bucket` 一次带回 COUNT/SUM 系列、Rust 侧拆序列；分位按桶采样或 SQLite 端近似；month_mode 短路前置。

### S6 [P3] cron 日志侧 SQL 写法

- **场景**: `prune_old_runs` 对 job 的 runs 全量 `find().all()` 无 LIMIT 再 Rust 端 skip 取应删清单（量级小、有 run_id 索引，风险低，但写法应为只取应删行）；`list_logs` 的 `(run_id)` 索引不含 seq 排序列，单 run ≤2000 行内存排序可接受。
- **证据**: `src/cron/log_repository.rs:211-224`（全量取数）；`src/cron/log_repository.rs:189-196`（list_logs ORDER BY seq）
- **修法**: 按 started_at 找第 keep+1 旧的 run 后 `WHERE started_at < ?` 删除；(run_id, seq) 覆盖索引（低优先）。
