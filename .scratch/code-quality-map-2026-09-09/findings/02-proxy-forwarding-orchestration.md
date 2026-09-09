# FINDINGS · 02 proxy 转发编排与选路审查（2026-09-09）

范围：`lb.rs` / `route.rs` / `calls.rs` / `headers.rs` / `forward.rs` / `failover.rs` / `native.rs` / `usage_rank.rs` / `failure_recheck.rs` + `mod.rs`/`tests.rs` 域内测试；跨域接口抽查 dispatch.rs（record_failure/sse_response/dispatch_success 记账缝）、entity virtual_model、routes/openai_compat 过滤口径。方法：全文件逐行通读 + 跨文件流程追踪 + 关键行为对照磁盘逐条复核。清单模式：不改代码，条目供图后统一排期。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 02-01 | P2 | 错误分支/可观测性 | 成员终态失败零 tracing 日志：降级分支每条 warn、终态（末成员/重试关闭）分支只有落库没有日志 |
| 02-02 | P3 | 错误分支 | failover 尾部「理论不可达」兜底是死代码，且若被触发会对已落 degraded 行的请求重复落一行失败 |
| 02-03 | P3 | 简洁 | calls.rs 两函数尾段（第 2-4 层头组装 + opencode 注入 + host_of）逐字重复 ~20 行 |
| 02-04 | P3 | 性能 | Gemini 成员构建时 inline_remote_images 网络下载图片，failover 每成员重下（无跨尝试缓存） |
| 02-05 | P3 | 性能/健壮 | resolve_usage_map 第二层批量 DB 读失败被 unwrap_or_default 吞掉 → 全部落入真实抓取（无退避） |
| 02-06 | P3 | 日志量 | strategy 0/1 每请求 3 条 info 决策日志（用量明细/排序结果/选路结果），高 QPS 噪音（by-design，供复核） |
| 02-07 | P3 | 口径（需拍板） | 额度剔除致空候选/空成员请求 503 不落 request 表——额度门控拒绝是排障盲区 |
| 02-08 | P3 | 口径（需拍板） | forward_chat_direct 成功经 dispatch_success 清零供应商生产连续失败计数，失败却不计数——与注释「失败不触碰可用性状态机」不对称 |
| 02-09 | P3 | i18n | native.rs 错误文案 zh-only 硬编码（:62/76 等，F10 族内，此处仅锚点登记） |
| 02-10 | P3 | 简洁/常量 | route.rs:61 retry_enabled = fallback_strategy == 1 裸数字（entity 已定义 RetryEnabledMembers 常量） |
| 02-11 | P3 | 测试覆盖 | build_native_upstream_call 头组装零单测（tests.rs 只测 chat 侧 build_call_sync）；failover 终态分支行为无单测锁定 |
| 02-12 | P3 | 注释一致性 | usage_rank fallback 从 5h 层全层重扫，0% 窗口按数值判负而非「判平」——与层循环注释微矛盾；真实路径被 retain 预剔除保护，纯函数级不可达，仅注释/语义对齐 |

## 各条证据

### 02-01 终态失败零日志（P2，错误分支/可观测性）

`failover.rs` 中四个失败终态分支（解密失败 :190-223、请求构造失败 :236-268、上游调用失败 :273-306、HTTP >=400 :309-341）在 `retry_enabled && has_more` 为假时，都只 `record_failure` 后直接 `return MemberLoopOutcome::Failed(...)`——**没有任何 tracing 日志**。对照降级分支（同文件 :196-207/:241-252/:279-291/:315-328）每条都带 request_id/attempt_index/fail_reason 的 warn。全文件唯一 error 级日志在 :363-371 的「理论上不可达」尾部。后果：一次全部成员失败的请求在日志里只有入口侧的 LB 选路 info，失败原因只能查 request 表或让客户端回传——与生产排障经验一致（此前已确认的终态零日志残余）。建议：终态 return 前补一条与降级同形状的 error 级日志（request_id/末成员/status/fail_reason/尝试数）。

### 02-02 兜底死代码（P3，错误分支）

`failover.rs:355-383`：循环体所有分支要么 return 要么 continue（continue 仅在 has_more 时发生，必然有下一轮），尾部落不可达。其 `record_failure` 用未加后缀的 request_id——若因未来改动落入，会与前面的 `request_id-N` degraded 行重复且与 02-01 相反产生唯一一条错误日志（掩蔽问题）。建议：改为 `unreachable!()`（或保留仅 log 不落库的防御），避免双写语义。

### 02-03 calls.rs 头组装尾段重复（P3，简洁）

`build_upstream_call`（:55-77）与 `build_native_upstream_call`（:111-128）从 `headers.extend_from_slice(forwarded)` 到 opencode 会话头注入逐字重复（~20 行，含 merge_custom_headers/merge_template_default_headers/apply_protocol_auth_headers/opencode 注入四步 + 两处 `host_of`/URL 行）。差异仅 URL/body 构造与协议。建议抽 `assemble_outbound_headers(member, forwarded, upstream_host, api_key, opencode_session, request_id) -> Vec<(HeaderName, HeaderValue)>`（helpers 全部已在 headers.rs，收敛成本低）。

### 02-04 Gemini 远程图片 failover 重复下载（P3，性能）

`calls.rs:33-49`：Gemini 臂在每次成员构建时调用 `gemini::inline_remote_images`（真实网络下载，走代理）。failover 到下一成员（同一请求）时重新构建 → 同一批图片重复下载。图片 URL 天然幂等，建议 per-request 一次性下载缓存（或在 flavor 构建前预取一次）；成员多为同供应商不同模型时该路径罕见，P3。

### 02-05 resolve_usage_map DB 层失败静默降级（P3，健壮/性能）

`lb.rs:426-432`：第二层 `read_usage_cache_many` 失败 `unwrap_or_default()` → 视同无缓存 → 全部落入第三层真实抓取（`lb.rs:442-453` 并发 fetch_shared，直打厂商 API）。DB 瞬时故障会把一次选路放大成 N 家厂商真实请求。建议：DB 错误与「无缓存」区分——错误时保留内存层结果直接返回（抓取留待下一请求）。

### 02-06 strategy 0/1 每请求 3 条 info 决策日志（P3，日志量）

`lb.rs:256`（用量明细 info）+ `lb.rs:313`（排序结果 info）+ `route.rs:77`（选路结果 info）每请求 3 条 info；排序明细另有 debug（route.rs:69）。生产 RUST_LOG=info 下 ZCode/IDE 高频调用（usage 类虚拟模型）日志放大显著。决策日志语义 by-design（5e9df8f 统一形状），此处供复核：明细两条是否可降 debug、保留 info 仅选路结果。

### 02-07 额度空候选 503 不落 request 表（P3，口径，需拍板）

两条路径都不落 request 表：resolve `NoMembers`（成员空，route.rs:48-50 → 各自 503，**且零日志**——forward.rs:71-78/native.rs:123-129 直接 return）与 `forward_through_members` 空 ordered（lb.rs:273-302 retain 全剔除 → failover.rs:159-167 503，有 warn 日志）。请求真实发生且被额度门控拒绝，request 表无行 → 数据面板看不到门控拒绝（failover 400 族排障盲区；NoMembers 分支连日志都没有）。需拍板：是否给「额度耗尽 503」记失败行（可带 fail_reason=quota_exhausted 不入成功/失败统计口径，或单独标记）。

### 02-08 直连成功清零生产失败计数（P3，口径，需拍板）

`forward.rs` 注释（:146-147）「失败不触碰可用性状态机（后台试用不该累积生产供应商的连续失败计数）」——失败侧确实不调 note_member_failure；但成功侧经 `dispatch_success`（dispatch.rs:137 `failure_counter.reset`）**会清零**该供应商生产路径积累的连续失败计数。不对称：试玩成功一次即冲销生产熔断进度。需拍板：直连成功是否也应跳过 reset（或该行为视为「成功=健康信号」有意为之，仅补注释）。

### 02-09 native 中文文案（P3，i18n）

native.rs:77-81（请求体非法 JSON）、:91-95（缺 model）与 forward.rs:33-38（缺 model 400）均为 zh-only 硬编码；F10 族（proxy 域 Lang 消费 0）已登记，此处为具体锚点。清单内归 F10 批处理。

### 02-10 裸数字 fallback 常量（P3，简洁）

`route.rs:61` `retry_enabled = virtual_model.fallback_strategy == 1`；`entity/virtual_model.rs:25` 已定义 `RetryEnabledMembers = 1`（该枚举目前仅有此变体且生产无消费——F5 族注记）。建议 route.rs 消费常量（或 F5 批统一编号 taxonomy 时一并处理）。

### 02-11 测试覆盖缺口（P3，测试）

tests.rs（542 行）覆盖：头剥离清单/allowlist/custom_header 覆盖规则/opencode 注入/模板默认头（build_call_sync 单臂=chat OpenAI Compat）——但 `build_native_upstream_call`（原生头组装：黑名单全量透传路径）零单测；`forward_through_members` 终态分支（含 02-01 所述零日志行为）无单测（集成测试锁定响应/落库，未锁日志行为——若按 02-01 建议补日志，建议同步加单测断言）。域内 22 个 usage_rank 比较器测试质量高（含与 subscription_usable 一致性回归）。

### 02-12 usage_rank fallback 语义注释对齐（P3，纯注释级）

usage_rank.rs:76-84 `cmp_remaining_percent` 从 QUOTA_LAYERS[0]（5h）全层重扫——层循环（:28-43）对「某方该层无额度（剩余 0）」判平进入下一层，但 fallback 重扫时 0% 窗口按数值判负（cmp_window 无 0% 特判）。真实数据路径中 0% 供应商已被 order_members retain（subscription_usable）预剔除，比较器内不可达——纯函数/注释一致性观察，无实害；如需完全对齐可在 fallback 中跳过无额度层（或注释说明 0% 不可达的前提）。

## 已核验无问题区（避免后续票重复审查）

- headers.rs 四层组装与 spec 逐条相符：剥离清单两分类齐全（:22-55）、custom_header 协议保留名跳过+warn（:138-149）、黑名单优先于 allowlist（:103-105）、协议头 retain+push 覆盖（:205-212）、HeaderName 比较大小写不敏感无重复头风险、opencode 仅对 opencode host 且同名跳过注入（:70-77）。
- usage_rank.rs 比较器：FEFO 截止链/判平/兜底与既有测试与「最差窗口」口径一致（单测 22 个 + 与门控一致性回归 :489-540）。
- 空候选两路径守卫齐全（NoMembers→503 / empty ordered→503 文案区分额度语义），无 C1 时代 panic 残留。
- 降级行 request_id-N 后缀、终态行无后缀的落库区分正确；同一 provider 每请求仅计一次连续失败（counted HashSet，lb.rs:30-37）；成功清零计数语义在循环与 dispatch 双处执行（幂等）。
- 原生透传：接口类型严格门 + 成员协议防御过滤（native.rs:107-109）；非流式读体失败 502+落库与 chat 口径一致（:210-230）；流式中断按失败记账（StreamOutcome::from_parts 与 relay 统一）。
- failure_recheck.rs 经 lb.rs:47 转发失败链消费——「proxy 内 usage 桥」判定成立（01 盘点复核一致），本票无新增。
- 记账缝（dispatch.rs record_failure 字段/ttft_start 语义、sse_response Err 裸截断）与 03 票边界清晰，无越界调用。

## 拍板结论（2026-09-09，三项均已定）

- **02-07（落库记失败行）**：额度耗尽/成员为空导致的 503 落 request 表失败行（success=false，fail_reason 标注额度耗尽语义；接受其对失败率口径的影响）。实施批：NoMembers 与空 ordered 两分支补 record_failure（NoMembers 分支顺带补 warn 日志——原零日志）。
- **02-08（直连成功清零）**：保持现状不改行为——成功=健康信号，清零视为一次健康探测为有意设计；实施批仅在 forward_chat_direct 注释补全成功侧语义。
- **02-06（决策日志级别）**：strategy 0/1 的「成员用量明细」「排序结果」两条降 debug（lb.rs:256/:313），info 只保留选路结果（route.rs:77）；深排时临时调 debug 可还原完整决策链。
