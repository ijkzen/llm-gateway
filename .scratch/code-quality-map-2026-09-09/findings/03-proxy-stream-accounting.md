# FINDINGS · 03 proxy 流式转运与指标记账审查（2026-09-10）

范围：`relay.rs`（统一转运泵/Converter 门面/StreamOutcome/RecordCtx）/ `dispatch.rs`（dispatch_success/collect_stream_events/record_failure/sse_response）/ `metrics.rs`（Usage/StreamMetrics/RequestRecord/P4 单写者）/ `sse.rs`（SseSplitter/sse_frame）+ 域内测试（src/proxy/tests.rs、tests/proxy_integration/{protocol,responses_live,upstream_abort,failover}）。方法：四文件全量逐行通读 + 转换器状态接口（三协议 converter error/finish/usage 语义）与 native 记账交叉核对 + 测试覆盖盘点 + 性能/内存专项。清单模式：不改代码，条目供图后统一排期。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 03-01 | P2【已修复 2026-09-10】 | 逻辑正确性/客户端契约 | 带内错误事件（转换器 error 态）客户端仅收 [DONE] 无 error 帧——空/截断内容呈现「完整成功」假成功（已拍板修复） |
| 03-02 | P3【已修复 2026-09-10】 | 逻辑正确性/记账边界 | OpenAI 直通已转发 [DONE] 后的 teardown 读错误整单翻失败 + 补发 error 帧与二次 [DONE]——[DONE] 后字节属连接噪音 |
| 03-03 | P3【已修复 2026-09-10】 | 可观测性/错误分支 | 02-01 同族续点：dispatch.rs 四处 502 失败路径与 relay 流失败路径零 tracing（两文件 0 调用）——仅落 request 表 |
| 03-04 | P3 | 简洁/残余分叉 | 事件驱动循环双份：relay 泵内联循环 vs collect_stream_events 同语义独立实现，带内错误处理已在两份分叉（03-01 根因之一） |
| 03-05 | P3 | 健壮/诊断质量 | Anthropic/Gemini 非流式读体失败被 unwrap_or_default 吞成「解析上游响应失败：EOF」——真实原因丢失，文案误导排障 |
| 03-06 | P3 | 测试覆盖 | relay.rs/dispatch.rs 无内联单测：StreamOutcome 组合判定、accumulate_chunks 聚合行为、断开分支均无直接锁定 |
| 03-07 | P3 | 测试覆盖 | 带内错误客户端形状零测试（与 03-01 伴生，修复时补三协议回归） |
| 03-08 | P3 | 性能/健壮 | SseSplitter 缓冲无上限（无 `\n` 数据无限累积）——上游数据不可信，可被异常上游无限膨胀内存 |
| 03-09 | P3 | 口径/过滤边界 | usage-only 尾块过滤只认「空 choices+usage」形态，usage 并入非空终块的上游（DeepSeek 类）在客户端未请求时仍透传 usage |
| S3 | — | 归位遗留项重估 | request 表保留策略：保持现状不清理（已拍板），原 P1 读侧已被统计快照消除，登记为体积观察项 |

## 各条证据

### 03-01 带内错误事件客户端假成功（P2，逻辑正确性/客户端契约）【已修复 2026-09-10】

三协议转换器对上游 200 SSE 流内的错误事件（Responses `error`/`response.failed`、Anthropic `error`、Gemini `{"error":...}` chunk）统一置 `error` 态 + `finished`，`convert_event` 返回 `Ok` 且**不产出任何 OpenAI 错误帧**：

- responses/stream.rs:411-422（`response.failed`/`error` → self.error + finished，out 空）
- anthropic/response.rs:250-253（同形）
- gemini/response.rs:265-271（同形）

泵对 `Ok(空)` 无感，靠 `source.finished()` 退出循环；随后 relay.rs:321 `clean` 因 `source.error()` 为 Some 判负 → 跳过全部收尾注入 → relay.rs:389-395 `send_done` 恒发 `data: [DONE]`。客户端最终收到：此前已发的内容增量（若有）+ **[DONE]**——无 error 帧、无 finish chunk，表现为「完整成功」（空或截断内容）。DB 侧正确（outcome 经 source_err 记失败，b3d7600 已修 Err 路径），**客户端侧对称缺口未修**。对照：非流式同路径 dispatch.rs:488-492 已把 converter.error() 归为错误 → 502，无此问题；relay.rs:238-239 注释声称「错误收尾（error 帧 + [DONE]）由泵统一处理」——error 态未覆盖，注释与实现不符。

**拍板（2026-09-10）**：修复。Convert 事件源在 `convert_event` 后检查 `converter.error()`，发现即按 PumpStep::Failed 同形发 OpenAI error 帧 + [DONE]（与畸形事件/上游断流一致，客户端可抛错而非消费空完成）。实施批：relay.rs 一处补全 + 03-07 三协议回归测试。

### 03-02 已交付 [DONE] 后的 teardown 读错误翻转整单（P3，记账边界）【已修复 2026-09-10】

OpenAI 直通（TailSpec::Plain）把上游 [DONE] 帧原样转发后循环继续 `body.frame().await`——若此刻读侧报 Err（上游发完数据后 RST/超时，而非干净 FIN），relay.rs:289-297 置 upstream_err → 发 error 帧 → relay.rs:389-392 `send_done = upstream_err.is_some()` 为真 → **客户端在 [DONE] 之后又收到 error 帧 + 第二个 [DONE]**（若连接尚在），且整单记失败。`[DONE]` 是客户端可见流终结符，其后字节属连接 teardown 噪音，内容实际已完整交付。正常路径（hyper 对自定界 chunked 体无需再读即可 Ok(None)）不受影响，风险窗=close-delimited 或 RST 竞态。建议：Plain 源在已转发 [DONE] 后忽略后续读错误（记成功、不再发帧）；修复成本低，可与 03-01 同批评估。

### 03-03 dispatch/relay 失败路径零日志（P3，可观测性，02-01 同族续点）【已修复 2026-09-10】

relay.rs 与 dispatch.rs 全文件 **0 处 tracing 调用**（metrics.rs 写者失败 3 处 warn 除外）。02-01 锚定的零日志终态在 failover.rs 预 200 分支；本域是其续点：dispatch.rs 四处 502 失败路径（OpenAI 非流式读体失败 :142-163 / JSON 解析失败 :168-189、AG 非流式解析失败 :318-341、转换失败 :383-401）与 relay 泵内流失败（读上游 Err/转换 Err 分支）均只 `record_failure` + 回错误响应，无任何日志。一次「200 后读体失败」的请求在全链路日志里不可见（入口 LB info 之外）。建议并入 02-01 修复批（同形状 error/warn + request_id + fail_reason），02-01 批注中显式含本域 4 处 + relay 2 分支。

### 03-04 事件驱动循环双份（P3，简洁/残余分叉）

「读帧 → SseSplitter 拆分 → 事件源处理 → 内容打点 → 终结判定 → 错误归集」循环存在两份独立实现：relay.rs:282-320（泵内联，客户端流式）与 dispatch.rs:451-498 `collect_stream_events`（Responses 非流式客户端整流收集）。两者对 converter 错误态的处理已经分叉：collect 把 converter.error() 收为 error → 502（正确），泵只落库不给客户端错误外观（03-01 根因）——同一循环两处演化的实锤。native.rs:278-324 为第三份但语义确不同（原始字节不重帧、无转换错误概念），不并入。建议：03-01 修复后两份语义对齐即可，再评估是否把「驱动源」抽为共享原语（sink=发送帧 or 收集 Vec），非强制。

### 03-05 AG 非流式读体失败原因被吞（P3，健壮/诊断质量）

dispatch.rs:318 `let body = upstream::read_body(reply.body).await.unwrap_or_default();`——读体失败（超时/截断，UpstreamError）被吞成空体，落入 JSON 解析失败分支（:320-341），客户端 502 与落库 fail_reason 均为「解析上游响应失败：EOF while parsing a value」，真实原因（读取上游响应失败：…）丢失。对照同文件 OpenAI 直通臂显式区分读失败（:142-163）与解析失败（:168-189）。建议与 OpenAI 臂同形拆出读失败分支。

### 03-06 域内无内联单测（P3，测试覆盖）

- relay.rs（419 行）与 dispatch.rs（544 行）均无 `#[cfg(test)]`：StreamOutcome 三错误源 × disconnect 的 success/fail_reason 组合判定（relay.rs:113-147）、PumpSource 直通过滤/剥除分支（relay.rs:173-236）零直接锁定（仅经集成路径间接覆盖）。
- accumulate_chunks（dispatch.rs:17-117，纯函数）仅 src/proxy/tests.rs:44-71 一条单测（tool_calls index 剥离）；content 拼接/reasoning_details 聚合/finish_reason 取末块/首块 id·model·created 取首等行为靠 Responses 集成 happy path 覆盖。
- 客户端断开分支（disconnect → success=1 + 客户端提前断开）无任何测试锁定（集成 mock 不断开客户端）。
- 对照：metrics.rs tps 三单测、sse.rs 拆分三单测质量良好；畸形事件/断流/E1/E2 回归已锁（protocol.rs:189-222、upstream_abort 4 测试）。

### 03-07 带内错误零测试（P3，测试覆盖，03-01 伴生）

tests/ 无任何「200 SSE 流内错误事件」fixture：畸形事件回归只锁 parse-Err（protocol.rs:189-222 keyword "malformed-stream"），上游断流测试只锁 frame-Err（upstream_abort StreamAbort）。三协议带内错误（error/response.failed/{error} chunk）的客户端形状与落库无锁定——03-01 实施时补三协议回归（断言 error 帧 + [DONE] + 落库失败），防双份循环再次分叉。

### 03-08 SseSplitter 缓冲无上限（P3，性能/健壮）

sse.rs:18-62：`feed` 把输入 append 进 buffer，只有遇到 `\n` 才消费；上游持续发送无换行数据时缓冲无限增长（relay 与 native 两泵 + 各旁路 scanner 共用此类型，上游数据不可信）。正常上游每帧行长有界（<~100KB），风险=异常/恶意上游内存膨胀（且每帧还经 from_utf8_lossy → String 双份拷贝）。建议：行长度上限（超限即断流按读失败记账），低优先。

### 03-09 usage-only 尾块过滤形态过窄（P3，口径/过滤边界）

网关对 OpenAI Compat 流式恒向上游注入 `stream_options.include_usage`（convert/openai.rs build_request_body，单测 :197-214 锁定），客户端未请求时在泵内过滤 usage 尾块（relay.rs:185-187）。过滤判定 is_usage_only_chunk（openai.rs:113-122）只认「choices 空数组 + usage 对象」——OpenAI 规范的独立尾块形态。usage 并入**非空 choices 终块**的上游（DeepSeek 类兼容厂商行为）时过滤失效，客户端未请求也收到 usage（浅泄漏，非安全项）。集成测试 openai_passthrough_stream_hides_usage_chunk_unless_requested（protocol.rs:125）只锁规范形态。建议：清单内注记，扩展判定（无 delta 内容 + 含 usage 的末块）或文档化接受上游形态差异。

## 归位遗留项 S3：request 表保留策略重估【已拍板：保持现状不清理】

- **原状**：S3 原判 P1（codebase-audit-2026-09-08 FINDINGS.md S3）= request 表只增不减 + summary 默认全历史 O(N) 聚合无上限；曾实施「保留期每日清理默认 90 天」（a7d88b8）→ 2026-09-09 按用户要求撤销（revert 22bb5c8），FINDINGS 标注「保留策略另行决策」→ 归位本票。
- **重估**：读侧驱动已消除——ADR-0021 统计快照上线后 summary/rank/metrics/insight 全走闭桶快照（快照缺失闭桶才实时兑底，只扫缺口段），request 全史线性读只剩 request_logs 明细页。余下成本 = 写侧存储增长（每行 ~0.4KB 级 + failover 尝试行）+ 备份体积 + 超长窗 SQL 变慢；快照表（EAV）同步只增，量级小于 request 行。全史明细对排障/对账有回溯价值（快照为聚合，明细是真相源）。rollup 不成立：快照就是聚合等价物，再建归档与快照重复。
- **拍板（2026-09-10）**：保持现状不清理。图后实施批登记为体积观察项（如 request 行数 >1 千万或 DB 文件 >2GB 时重新评估保留期），不实施自动清理；恢复形态若将来需要，revert 22bb5c8 的完整实现（env 配置 + 每日任务）可直接复原。

## 已核验无问题区（避免后续票重复审查）

- **客户端断开记账**：disconnect → success=1 + fail_reason=「客户端提前断开」（relay.rs:112/136-146）与 entity/request.rs:56 文档（「成功但客户端中断」）一致，属文档化设计——客户端取消不算上游失败，仅补记原因；已核验 stats 成功率口径不受影响（fail_reason 非空但 success=1 的展示语义明确）。
- **落库单点**：泵尾/各臂/record_failure 全部经 `RequestRecord.insert` → P4 单写者队列（metrics.rs:177-282），无直插绕过；写者失败有 warn（metrics.rs:229/241/277）；通道满回退独立 spawn 不阻塞转发路径；写者跑独立线程 runtime（测试短命 runtime 不丢队列行）。
- **失败行区分**：本域只产终态行（request_id 无后缀），降级行 -N 后缀属 failover（02 结论），无重复/双写路径。
- **ttft/tps 口径**：与 entity/request.rs 文档逐条一致——ttft 起点=建连开始（新连接）/请求发出（复用）、流式分母=ttft+输出耗时、非流式分母=end−ttft_start（含建连，注释明示）、output_tokens_time 压缩窗口不兜底（entity 注释：短回复单 chunk 突发窗口压缩到几十 ms 甚至 0 属有意保留原始值）。
- **泵骨架收拢成效**：四协议流式臂全部经 relay_stream 单一入口（dispatch.rs:218-440 各臂为薄壳）；PumpSource 两源 / TailSpec 三分支枚举封闭（新增协议只需加变体）；无死代码（Converter::final_chunk 仅 Gemini 有值、Responses/Anthropic None 属接口对称；chunk_has_content/strip_reasoning_delta 被两泵共享）。错误优先级 upstream > convert > source 稳定；error 帧统一 api_error/upstream_error 形状。
- **reasoning 剥除/回传**：reasoning_exclude 在泵逐帧剥 delta（relay.rs:100-108/203-205）与 dispatch 非流式剥 message（dispatch.rs:6-14/289-291/358-360）双侧一致；思考块无损透传/加密载体不透明搬运属 05 票转换器域，接口消费方（Converter 门面 6 方法）已收敛。
- **02/04/05 边界**：relay↔convert 接口面干净（Converter/TailSpec/PumpSource 均在 relay 单点声明）；dispatch_success 内 failure_counter.reset（dispatch.rs:137）属 02-08 已拍板（直连成功清零保持现状）；record_failure/sse_response/collect_stream_events 被 forward/native/probe 复用，无越界调用。
- **sse.rs 拆分器**：字节下标区间实现无逐行分配（已修 O(n²)）；CRLF/多 data 行/注释与 event: 行忽略/跨帧残余/[DONE] 特判均有单测锁定；切片边界安全（`\n`/`\r` 为 ASCII 单字节，不可能落在多字节字符中间）。

## 性能/内存轮结论

无 P1/P2 级问题。正向确认：mpsc(32) 有界背压（发送端 await，客户端慢/断开不放大上游读取内存）；单写者批写（50 行/200ms 空闲冲刷）避免高 RPS 落库风暴；流式路径峰值内存=单帧（不缓冲整流，Responses 曾整条缓冲已改 live 转发）；转换臂 per-event 2-3 次分配（Bytes→String→sse_frame）量级可接受。P3 级：03-08（splitter 无上限）、03-04（非流式聚合整流缓冲属客户端形态固有，非回归）。

## 实施进度

- **03-03 已修复**（随 02-01 同批）：`dispatch.rs` 新增 `log_dispatch_failure` 并在四处 502 失败路径（OpenAI 读体/解析、AG 非流式解析、转换失败）调用；`relay.rs` 三处（上游读流失败、事件转换失败、`StreamOutcome` 终态失败）补 warn，均带 request_id/provider/model/fail_reason。

## 实施批补充（2026-09-10）

- **03-01 已修复**：`PumpSource::on_event` 的 Convert 臂在 `convert_event` 后检查转换器 error 态，发现即按失败收尾（有内容增量时先发增量帧，新增 `PumpStep::FramesThenFailed`）——泵对两种形态都补发 error 帧 + [DONE]，客户端不再收到「截断但正常结束」的假成功。回归测试 `anthropic_stream_inband_error_sends_error_frame_and_records_failure`（先红后绿已验：无该修复时只收 [DONE]）。
- **03-02 已修复**：`OpenAiStreamScanner` 记录 `saw_done`，泵在 `TailSpec::Plain` 且已见 [DONE] 后把读错误视为连接 teardown 噪音（debug 日志、按成功结束）——不再出现 [DONE] 之后的第二个 error 帧与第二个 [DONE]，也不再把已完整交付的整单记失败。
