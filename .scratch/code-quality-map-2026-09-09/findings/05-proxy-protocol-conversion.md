# FINDINGS · 05 proxy 协议转换审查（2026-09-10）

范围：`convert/` 全目录（mod.rs 门面/共享载体 + openai.rs 直通 + responses/{request,stream} + anthropic/{request,response} + gemini/{request,response,images} + 三协议 facades）及域内测试（anthropic/tests 30 例、gemini/tests 22 例、responses/tests 21 例、openai.rs 7 例、mod.rs 内联 16 例，合计 96 例）。方法：全目录逐行通读（~3800 行源码）→ 消费方交叉核对（relay.rs 泵/TailSpec、dispatch.rs RequestFlags、calls.rs 单点调度、native.rs 旁路 scanner、probe.rs 复用）→ 历次审计边界圈定（protocol-conversion-audit FINDINGS 09-05 整改状态、openrouter 01-08 批、zcode 8 工单、架构轮 6 候选——已修项不复查，仅核验未回归）→ 测试盘点 + 性能专项。清单模式：不改代码。**本票无 P1**；两项 P3 级取舍已走拍板（05-01/05-03）。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 05-01 | P3·安全观察【已拍板：保持现状记观察】 | 逻辑/安全 | Gemini 远程图片下载任意 http(s) URL 无私有地址/元数据防护（SSRF 通道）+ 先整读后 20MB 校验（恶意 URL 内存放大） |
| 05-02 | P3【已修复 2026-09-10】 | 简洁/死代码 | `collect_tool_call_names` 计算即弃两处（anthropic/request.rs:13,94 `let _ = tool_names`、responses/request.rs:22,82 同）——仅 gemini 真用 |
| 05-03 | P3·口径【已拍板：保持现状 + 实证条目】 | 逻辑/口径 | OpenAI Compat 直通对原生思考开关形态（thinking.type/enable_thinking）保留原样又注入归一 reasoning_effort——双写语义未实证 |
| 05-04 | P3【已修复 2026-09-10】 | 逻辑/JSON 模式 | Anthropic json 模式：非流式丢弃同响应 text（模型违规 preamble）；流式多 json 工具缓冲 HashMap values() 无序 flush |
| 05-05 | P3【已修复 2026-09-10】 | 简洁/死字段 | `GeminiStreamConverter.model` 只写不读（gemini/response.rs:273-275，字段 :220）——旧账 B8（流式 model 死代码）形态确认仍在 |
| 05-06 | P3【已修复 2026-09-10】 | 测试覆盖 | `sanitize_gemini_schema` 复杂清洗逻辑（类型数组/大写/format 白名单/键过滤/空 properties 摘除）零单测 |
| 05-07 | P3【已修复 2026-09-10】 | 测试覆盖 | images.rs 远程图片下载全路径零测试（成功/非白名单 mime/超 20MB/失败移除/代理/移除序号）——B1 修复未带测试 |
| 05-08 | P3【已修复 2026-09-10】 | 测试覆盖 | reasoning_details 总量钳制（128 项/512KB 截断，mod.rs:63-64）无测试——畸形项过滤有测、上限截断路径无 |
| 05-09 | P3【已修复 2026-09-10】 | 性能/一致性 | images.rs 每次 chat 请求重建 reqwest client（TLS 会话不跨请求复用，usage 侧 P5 已把 client 挂 AppState，同型未落）+ 多图顺序下载 |
| 05-10 | P3【已登记观察 2026-09-10】 | 性能观察 | Responses 流式转换器为去重保留全文副本（streamed_text/reasoning/args 三 map O(总输出)）——内存随输出增长，03 轮「峰值=单帧」结论在 Responses 转换器不成立 |

## 各条证据

### 05-01 Gemini 远程图片下载：SSRF 通道 + 无界读取（P3·安全观察）【已拍板：保持现状记观察】

gemini/images.rs `inline_remote_images`（calls.rs:37 在构建 Gemini 请求体时调用）：把 contents 中任意 `http(s)://` fileData 图片**服务端下载**后转 inlineData（:60-122）。要点：

- **无目的地防护**：`is_remote_http_uri`（:8-10）只判前缀；下载目标可达回环/链路本地/私有网段/云元数据地址（169.254.169.254 等）。reqwest 默认跟随重定向——公网 URL 可 302 到内网。攻击面 = 持有效 Bearer key 的调用方（/v1 门禁内）可盲扫网关所在网络（Docker 桥、内网、云元数据）：`response.bytes()` 已发出请求本身即探测；若内网端点返回白名单 mime（image/jpeg 等 4 种）的图片，内容经 Gemini 模型描述后可回显——受限但真实的泄露通道。管理面（session）无此面（test_model 不带图）。
- **先整读后校验**：:47-50 `response.bytes()` 全量读入内存后才判 20MB（:4 MAX_INLINE_IMAGE_BYTES）——恶意端点返回无限流时内存随响应体增长（reqwest 无 body 上限），单请求内存放大；仅 15s 超时兜底（:20）。
- **失败静默**：下载失败仅 warn 并移除 part（:96-99/117-119），客户端无感知（B1 已拍板行为，此处不重评）。

**拍板（2026-09-10）**：保持现状，记观察。理由：/v1 Bearer 门内 key 由管理面单用户发放，信任边界=key 持有者；不引入配置面。登记观察触发条件：key 分发面扩大（多用户/对外）、网关网络内出现高价值内网服务、或下载放大被利用；届时默认解 = URL host 解析后拒绝回环/链路本地/私有段/元数据地址 + Content-Length 预检/流式截断 20MB + 重定向次数与目的地同规则。

### 05-02 collect_tool_call_names 计算即弃（P3，简洁/死代码）【已修复 2026-09-10】

`collect_tool_call_names`（mod.rs:462-481，遍历全对话 assistant tool_calls 建 id→name 映射）在两处 request builder 中计算后从未消费：

- anthropic/request.rs:13 计算 → :94 `let _ = tool_names;`（注释自认「Anthropic 以 id 关联」）
- responses/request.rs:22 计算 → :82 `let _ = tool_names;`（同）

仅 gemini/request.rs:132 真用（tool 结果反查 functionResponse.name）。默认解：删除两处计算与 `let _`，调用点仅留 gemini。

### 05-03 OpenAI Compat 直通思考参数双写（P3，口径）【已拍板：保持现状 + 实证条目】

openai.rs:26-33：`chat_reasoning(chat).enabled()` 时向直通体注入归一 `reasoning_effort` 并删除 OpenRouter `reasoning` 对象。客户端思考形态分四种，双写只发生在原生开关形态：

- 客户端带 OpenRouter `reasoning` 对象 → 删对象 + 注入 effort：正确归一（上游不认对象）。
- 客户端带顶层 `reasoning_effort`（含 none）→ 注入同值：幂等无害。
- 客户端带 `thinking.type: enabled`（DeepSeek 系）或 `enable_thinking: true`（百炼系）→ **原生键保留透传 + 注入 reasoning_effort: medium**——上游同时收到两个思考参数。语义未实证：DeepSeek 类上游若同时接受 thinking.type 与 reasoning_effort，档位 medium 可能与客户端语义叠加/冲突；若上游严格校验未知字段则 400（reasoning_effort 已实测现有全部供应商兼容，故概率低，主要风险是语义叠加）。
- 开关为 disabled → Disabled 不注入，原生键透传：无双写。

**拍板（2026-09-10）**：保持现状（双写同时覆盖「上游只认 effort」与「上游只认开关」两类理解，删除注入反而会让 OpenAI 官方类上游收不到思考意图），在实施批登记实证条目：对 DeepSeek 类真实上游发 thinking.type=enabled + reasoning_effort=medium 组合请求验证行为；若实证冲突再改为「原生形态存在时不注入」。

### 05-04 Anthropic json 模式：text 丢弃与多缓冲乱序（P3，逻辑/JSON 模式）【已修复 2026-09-10】

- **非流式**（anthropic/response.rs:86-89 累积 text → :134-135 json_tool_output 分支整条消息替换为 json content）：模型违规先输出 preamble 文本再调 `__structured_output__` 工具时，文本静默丢失（仅 json 到达客户端）。同响应内流式路径（:424-432）无此问题（text 已随 delta 发出）。
- **流式**（:424-432）：`json_mode_buffers.values()` 对 HashMap 无序迭代 flush——一次响应内模型多次调用 json 工具（多块独立 json）时输出块顺序不保证（HashMap 迭代序），多 json 结果拼接顺序可能错乱。单工具调用是常态，风险窗=多调用。

默认解（实施批）：非流式 json 分支把 text 并入响应（置于 json 之前或作为 content 拼接需定语义——倾向 json 工具输出优先、text 仅兜底日志）；流式 flush 按 block_index 排序。

### 05-05 GeminiStreamConverter.model 死字段（P3，简洁；旧账 B8 族）【已修复 2026-09-10】

gemini/response.rs:273-275 把上游 `modelVersion` 写入 `self.model`（struct 字段 :220，init :234），但全文件无任何读取——所有 chunk 均以 `requested_model`（请求别名）构造（:291/300/316/329 等）。protocol-conversion-audit B8「流式 chunk 的 model 字段死代码」09-05 记 nit 未修，此形态确认仍在（字段名 model + 写入点保留）。默认解：删字段与写入（别名口径由 requested_model 单点承担，与 D2 决策一致）。

### 05-06 sanitize_gemini_schema 零单测（P3，测试覆盖）【已修复 2026-09-10】

gemini/request.rs:302-305 入口 + :307-403 实现：类型数组取首非 null + 置 nullable、类型名大写、format 按类型白名单过滤（STRING→enum/date-time 等）、键白名单 retain（GEMINI_SCHEMA_KEYS 22 键）、空 properties 摘除、anyOf 递归。这是本域最复杂的纯函数之一（OpenAPI JSON Schema → Gemini Schema 双向降级），gemini/tests.rs 26 例中**零 schema 用例**（盘点确认：无 sanitize/schema 测试名），inline_defs 仅 mod.rs 一例。默认解：补字段级用例（类型数组、$ref 内联、format 摘除/保留矩阵、未知键摘除、空 properties、深度上限 16 截断）+ 与真实 Gemini 拒绝信息对照。

### 05-07 images.rs 全路径零测试（P3，测试覆盖）【已修复 2026-09-10】

gemini/images.rs（134 行）无任何 `#[cfg(test)]`，tests/ 下零命中（inlineData/下载路径无集成测试）——B1 修复（http(s) 图下载内联，v0.1.11）未带测试。未锁定行为：白名单 mime 判定、>20MB 拒绝、下载失败移除 part 且不位移（:103-121 倒序应用逻辑）、代理模式（build_image_client :21-24）、客户端构建失败全移除（:82-91）。默认解：本地 mock HTTP server（监听随机端口，images 的 reqwest 直连无需 override 缝）起集成/单测覆盖上述分支——现改现回归风险最高的文件之一。

### 05-08 reasoning_details 钳制上限无测试（P3，测试覆盖）【已修复 2026-09-10】

mod.rs:63-64 总量上限（128 项 / 512KB）+ :74 `take(REASONING_DETAILS_MAX_ITEMS)` + :92-96 超字节 break 截断——客户端可回传的防滥用闸门（回传数据最终进上游请求体）。畸形项过滤有测（:695-710），但**两项上限的截断路径零测试**（128 项后静默丢弃、512KB 后 break）。默认解：补两项上限用例 + 顺序保持断言（签名链不允许重排的守卫已在 :67 注释，无测试）。

### 05-09 images.rs 每请求重建 reqwest client（P3，性能/一致性）【已修复 2026-09-10】

images.rs:19-26/82：每次 chat 请求（calls.rs:37 调 inline_remote_images）新建 reqwest Client——TCP/TLS 会话随 drop 全丢，图多时同请求内虽共享一个 client（✓），但跨请求不复用。usage 侧同问题曾在 09-08 P5 修复（client 挂 AppState 按 proxy 维度缓存，见 usage/http.rs），images 侧未落同款。附带：多图顺序下载（:93-102 逐个 await），N 图最坏 N×15s。默认解：client 按 proxy 维度缓存（AppState 或静态 OnceLock 按 proxy addr key）+ 可选并发下载（数量少，非必须）。

### 05-10 Responses 转换器全文保留（P3，性能观察）【已登记观察 2026-09-10】

responses/stream.rs:48-50 `streamed_text/reasoning/args` 三 map 为 output_item.done/completed 回放去重保留**全文副本**直到转换器 drop（流结束）——内存 O(总输出)（如 100k token ≈ 数百 KB + 推理文本双份）。这是「completed 回放只发缺失后缀」设计（:154-207 missing_suffix）的固有代价，保证内容至少一次。03 轮性能结论「流式路径峰值内存=单帧」在 Anthropic/Gemini 臂成立（无回放），Responses 臂不成立——修正记录，量级无害（百 KB~MB），无需动作，仅在超长输出（>1MB 级）场景回看。

## 历次审计遗留衔接（不重记，仅确认现码状态与跟踪位置）

protocol-conversion-audit FINDINGS「仍未修复」清单项，现码核对仍在、跟踪点在 09-05 整改状态段（随实施批统一处理）：

- **A7**：tool_choice none + disable_parallel_tool_use 组合（anthropic/request.rs:278 仍无条件附加 disable_parallel_tool_use）——官方 schema 对 none 不接受该键，仅 parallel_tool_calls=false ∧ tool_choice=none 组合触发。
- **C4**：Responses max_output_tokens 无最小值 16 钳制（responses/request.rs:99-101 原样透传）。
- **C5**：tools/response_format 的 strict 不透传（responses/request.rs:122-140 转换只取 name/description/parameters；OpenAI 官方 strict function 语义丢失）。
- **B8/D3**：流式 model 死代码（本票 05-05 现码确认）、chunk `created` 每块取当前时间（mod.rs:333/348）——nit 级。
- **代际立项（图外）**：A2 adaptive thinking、O3 Gemini 3 thinkingLevel、O4/O5 effort 预算换算——产品向，本图 Out of scope 已划。

## P3 实施批（2026-09-10）

- **05-02 已修复**：删 `collect_tool_call_names` 在 Anthropic 与 Responses 两个请求构建器中的计算与 `let _ =`（两处注释自认不用），仅 Gemini 真用；两处 import 同步收敛。
- **05-04 已修复**：① 非流式 json 模式下模型违规输出的 preamble 文本不再丢弃（拼在 json 输出之前）；② 流式多 json 工具缓冲 flush 改按 block_index 升序（HashMap 迭代序不定会致块序错乱）。补回归 `anthropic_json_mode_keeps_preamble_text`。
- **05-05 已修复**：删 `GeminiStreamConverter.model` 字段与其 modelVersion 写入（全文件无读者，所有 chunk 用 requested_model）；补测试 `stream_chunks_always_use_requested_model_alias` 锁别名口径。
- **05-06 已修复**：新增 7 例 `sanitize_gemini_schema` 单测（类型数组取首非 null、大写、format 类型白名单、未知键摘除、空 properties 摘除、anyOf/items 递归、深度 16 上限）。
- **05-07 已修复**：新增 2 例图片路径测试（非白名单 mime 拒绝不产 inlineData、多图混合成功+失败时下标不位移）。既有 3 例（成功下载/失败移除/GCS 与 Files API 不处理）保留。
- **05-08 已修复**：新增 3 例 reasoning_details 上限测试（128 项截断且顺序从首项、512KB 截断、顺序保持断言）。
- **05-09 已修复**：`images.rs` 的 reqwest client 由每请求重建改为按代理维度缓存（进程级 OnceLock，与 usage/http.rs 同款），TCP/TLS 会话跨请求复用；多图并发下载未做（图少，非必须）。
- **05-10 已登记观察**：Responses 流式转换器为「completed 回放只发缺失后缀」保留全文副本，内存 O(总输出)（百 KB~MB 级）；量级无害，超长输出（>1MB）场景回看。
- **旧账落地（A7/C4/C5）**：① A7 `tool_choice=none` 时不再附 `disable_parallel_tool_use`（官方 schema 拒绝该组合）——实现时发现初版误吞整个 tool_choice，由新测试 `none_tool_choice_omits_disable_parallel_tool_use` 当场抓出并修正；② C4 Responses `max_output_tokens` 低于 16 钳到 16；③ C5 Responses 的 tools 与 response_format 的 `strict` 字段透传。各补 1-2 例单测。

## 已核验无问题区（避免后续票重复审查）

- **reasoning_details 载体全链路**：构造（reasoning_text_detail 签名空→null / reasoning_encrypted_detail 原样搬运，mod.rs:22-53）、请求侧校验（非对象/type 前缀/format 缺失丢弃 + 顺序保持，:68-100）、三协议注入只认自家 format（anthropic:305-334 / responses:189-209 / gemini:99-122 各自 filter format + type）+ OpenAI 直通剥离（openai.rs:16-22）——格式不匹配项互不污染，跨厂商 failover 后回传按 format 匹配不误挂。
- **思考关闭三态 × 四协议出站**逐一核对：Anthropic 缺省关→Unspecified/Disabled 都不写 thinking（request.rs:284-289）；Responses 缺省开→Disabled 显式 effort none（request.rs:116-119）；Gemini 缺省动态开→Disabled 写 thinkingBudget 0（request.rs:202-206）；OpenAI 直通→none 原样字节。`reasoning_effort:"none"`（zcode P5）在转换侧均已显式关闭，无「关不掉」路径。
- **工具轮签名回传**：Anthropic 无签名即跳过（不可回传的语义正确，:312-326）；Responses encrypted_content 缺失跳过 + output_item.done/completed 双路径去重（reasoning_detail_captured，stream.rs:251-253）；Gemini extra_content 与 reasoning_details 双路径按 index 去重（request.rs:92-122）——03 审计「思考块无损透传」接口面在此侧干净。
- **思维格式输出**：thinking→reasoning_content delta + details 块收尾发帧顺序（Anthropic content_block_stop / Gemini part 序 / Responses done+completed），detail index 递增单一来源；reasoning_exclude 剥除在 relay 泵统一（relay.rs:203-205，03 已核）。
- **finish_reason 全表 + 工具收尾优先**：C1 语义三协议一致（tool 存在即 tool_calls），原生值 native_finish_reason 透传 + 32 字符截断（mod.rs:357-382）；Gemini 流中 blockReason 覆盖 finishReason 为 content_filter（:348-352）；异常终态（Anthropic 无 message_delta、Gemini 无 finishReason）由 relay 尾补 finish（relay.rs:360-372，03 已核）。
- **usage 归一与旁路扫描等价**：Anthropic 转换器 start⊕delta 合并（response.rs:397-417）与 native 旁路 scanner 键覆盖合并（:505-520）两种实现今天等价（input 缺省回退 start、cache 只算 read、output 取 delta）——拷贝族漂移风险已记 03 教训，此处核对一致；Gemini/Responses extraction 单点共享。
- **空内容兜底族**：user/tool_result 空 → " "（anthropic:48-50/100、gemini:43-45/142-149 包装 result）、functionResponse 非对象包装、functionCall args 非法 JSON→{}、images 非白名单/失败移除——上游 400 面都有兜底。
- **单测面**：96 例覆盖强（三协议 73 例：思考档位/互斥采样参数/签名双写/回放去重/畸形 tool 输出/scanner 合并/分片 feed；openai 7 例；mod.rs 16 例覆盖 chat_reasoning 形态矩阵 8 例、载体校验、usage json），03 的 responses_live/upstream_abort 集成回归在域外已锁。缺项见 05-06/07/08。
- **模块间调用**：Converter 门面 6 方法（new/convert_event/usage/is_finished/error/has_finish）三转换器对称、final_chunk 仅 Gemini（relay 尾统一补 finish）；RequestFlags（json_mode_tool/thinking_dropped）dispatch.rs:131-132 消费 → with_thinking_dropped_header 客户端可见（:235/259/310/378）；build_request_body 四协议同签名 calls.rs 单点调度；scanner 1:1 寄生协议文件供 native 旁路（无第三份拷贝）——03「relay↔convert 接口面干净」从 convert 侧复核成立。

## 性能/内存轮结论

无 P1/P2。正向：四转换器逐事件处理无整流缓冲（非流式收集在 dispatch 侧按需）；json 模式缓冲有界于响应长度；每 chunk 分配 2-3 次（Bytes→String→sse_frame）与 03 同量级。P3 观察：05-09（images client 每请求重建 + 顺序下载）、05-10（Responses 回放去重全文保留，修正「峰值=单帧」的泛化结论）。结论：转换域内存形态适合当前输出量级（百 KB 级保留无害），无需结构性改动；若未来支持超长输出（>1MB）再评估 Responses 回放策略（改为仅记 emitted 长度 + 偏移比对）。
