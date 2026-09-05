# 上游协议转换核对报告

- 日期：2026-09-05
- 范围：`src/proxy/convert/`（openai/responses/anthropic/gemini + mod）与 `src/proxy/mod.rs` 管线中与格式转换相关的部分
- 方法：先核对四家官方接口文档（OpenAI Chat Completions / OpenAI Responses / Anthropic Messages / Gemini generateContent）与 LiteLLM main 快照实现，再逐字段比对本地实现

## 官方依据来源

| 协议 | 来源 |
| --- | --- |
| OpenAI Chat | `openai/openai-openapi` openapi.yaml v2.3.0 + platform.openai.com API 参考（agent 调研） |
| Anthropic | docs.claude.com `/en/api/messages`、`/en/docs/build-with-claude/extended-thinking`、`/en/docs/build-with-claude/thinking`（本次直接抓取原文核对）+ 官方文档全文调研（agent，含 anthropic-sdk-python/-typescript 的 OpenAPI 生成类型交叉核对） |
| Gemini | `googleapis/googleapis` v1beta proto（content.proto / generative_service.proto）+ `googleapis/python-genai` types.py + ai.google.dev 参考（本次直接抓取核对） |
| OpenAI Responses | 官方事件/字段以 LiteLLM 桥接实现与已知规格交叉印证（Responses 文档 agent 因并发限制失败，此协议的官方原文核对覆盖度略低，见各条目标注） |
| 参照实现 | LiteLLM main 快照（/tmp/litellm），anthropic/gemini/responses 三方向转换全表（agent 调研，含 file:line） |

## 严重度统计

- 高（会直接 400 或破坏工具调用语义）：4 项 — A1、A2、B1、C1
- 中（特定组合 400 / 行为偏差明显）：5 项 — A3、A4、B2、B4、D2
- 低 / 备注：13 项

## 整改状态（2026-09-05 更新）

**已修复并随 v0.1.11 发布 + 本地部署**：A1（budget≥1024 钳制）、A3（thinking 互斥部分：temperature 仅保留 =1、top_p 丢弃；temperature>1 的范围越界按 LiteLLM 策略仍交上游 400）、A4（强制 tool_choice 降级 auto）、A5（pause_turn→stop、compaction/model_context_window_exceeded→length）、B1（http(s) 图片下载转 inlineData，失败摘除）、B3（presence/frequency penalty）、B4（blockReason→content_filter）、B5（functionResponse/args 非对象包装）、B7（UNEXPECTED_TOOL_CALL 等新枚举）、C1（output 含 function_call → finish_reason tool_calls）、C3（移除默认 instructions）、D1（未请求 include_usage 时过滤 usage 尾块）、D2（Responses 改用请求别名；OpenAI 直通仍为上游模型名，字节直通设计如此）。

同批附带：多轮 tool_use 历史无 thinking 块时丢弃 thinking 参数（防 "Expected thinking or redacted_thinking" 400，LiteLLM 同款）、user→metadata.user_id、流式 usage chunk 统一含缓存明细、非流式 tool_calls 去 index、Gemini 工具名反查失败告警、Responses refusal.delta/.done 处理。

**仍未修复**：A2（adaptive thinking，需模型代际信息，待拍板单独立项）、A6（cache_creation 计入命中口径的取舍）、A7（tool_choice none + disable_parallel_tool_use 未知字段）、B2（thoughtSignature 透传，建议单独立项）、B8（流式 model 死代码等 nit）、C2（Responses 出站对流式客户端全量缓冲）、C4（max_output_tokens<16）、C5（strict 透传）、D3（chunk created 漂移）、D4 备忘。

---

## 一、Anthropic 方向（src/proxy/convert/anthropic.rs）

### A1【高】reasoning_effort="minimal" → budget_tokens=128，低于官方最小值 1024，上游 400

- 位置：`mod.rs:18-28`（`reasoning_budget`）+ `anthropic.rs:249-259`（`map_thinking`）
- 本地：`"minimal" => 128`，`map_thinking` 只要 `max_tokens > 1024` 就原样下发 `{"type":"enabled","budget_tokens":128}`
- 官方：`budget_tokens` "Minimum of 1,024 tokens. **The API rejects smaller values.**"（extended-thinking 页原文），且必须 `< max_tokens`
- LiteLLM：minimal → `max(128, 1024) = 1024`（`constants.py` `ANTHROPIC_MIN_THINKING_BUDGET_TOKENS=1024` 钳制）
- 影响：任何带 `reasoning_effort:"minimal"` 的请求打到开启 thinking 的 Claude 上游直接 400
- 建议：minimal 钳到 1024（或与其他协议一致地视 minimal 为「不开启 thinking」）

### A2【高】thinking 仅支持 `{type:"enabled"}`，Claude 4.7+/5.x 模型直接 400；未跟进 adaptive thinking

- 位置：`anthropic.rs:249-259`
- 官方（extended-thinking 页原文）：
  - "Extended thinking (`thinking.type: "enabled"` with `budget_tokens`) is **deprecated on the Claude 4.6 models**（请求仍成功）。**Claude 4.7 and later models do not support it and reject requests that use it, returning a 400 error.**"
  - 迁移映射：`thinking:{type:"adaptive"}` + `output_config:{effort:"low"|"medium"|"high"|"xhigh"|"max"}`
- 本地：只会发 `enabled+budget_tokens`，`reasoning_budget` 档位（128/1024/2048/4096/8192/16384）与官方 `output_config.effort` 枚举（low/medium/high/xhigh/max）对不上
- 影响：新 Claude 模型上凡带 `reasoning_effort` 的请求 400；网关无模型代际判断依据（`provider_model.reasoning` 只是布尔）
- 建议：短期至少把 400 错误透出（现在会走 failover 烧掉其他成员）；中期按模型名代际分流 enabled/adaptive，或全部改用 `adaptive+output_config.effort`（仅对 4.6+ 生效，旧模型会 400，需要模型信息）

### A3【中】temperature/top_p 照传：范围越界与 thinking 互斥两重 400 风险

- 位置：`anthropic.rs:201-206`（temperature/top_p 无条件拷贝）
- 官方两层约束：
  1. **范围**：Anthropic temperature "Ranges from `0.0` to `1.0`"（API 参考原文）——OpenAI 允许 0..2，客户端传 1.5 在任何 Anthropic 请求上直接 400（与 thinking 无关）；
  2. **thinking 互斥**（thinking 页原文）："On older models, the restriction applies only while thinking is on: **`temperature` and `top_k` are incompatible with thinking**, and `top_p` is allowed at values between 0.95 and 1."（新模型 4.7+/5.x 则任何时候非默认采样参数都 400，见 A2）
- LiteLLM：透传适配层显式 `optional_params.pop("temperature")` when thinking enabled（"Anthropic rejects any temperature other than 1 while extended thinking is enabled"）；范围本身不钳制（交上游 400）
- 影响：`temperature:0.7 + reasoning_effort:high` 这类常见组合（agent 框架常见）在 Claude 4.5 及更早 thinking 模型上 400；temperature>1 则任何时候都 400
- 建议：钳制/丢弃 temperature>1（超范围）；thinking 开启时丢弃 temperature（或钳到 1）、top_p 不在 [0.95,1] 时丢弃

### A4【中】thinking 开启时 tool_choice any/tool 未降级，组合 400

- 位置：`anthropic.rs:154-155`（`map_tool_choice` 与 `map_thinking` 独立计算，互不感知）
- 官方（thinking 页原文）："tool use with manual extended thinking only supports `tool_choice:{type:auto}`（默认）or `{type:none}`. Using `{type:any}` or `{type:tool}` **results in an error**"
- 本地仅在自家 json_mode 合成工具上规避了该冲突（`anthropic.rs:180-182` 注释即承认此约束），用户显式 `tool_choice:"required" + reasoning_effort` 仍会双发
- 建议：thinking 开启时把 any/tool 降级为 auto（或丢弃 tool_choice）

### A5【低】stop_reason 未显式映射 `pause_turn` / `model_context_window_exceeded`（均落入 default → "stop"）

- 位置：`anthropic.rs:329-343`（`normalize_stop_reason`）
- 官方 stop_reason 枚举全集（API 参考 + agent 调研确认，共 7 个）：`end_turn` / `max_tokens` / `stop_sequence` / `tool_use` / `pause_turn` / `refusal` / `model_context_window_exceeded`
- 本地显式映射 end_turn/stop_sequence/max_tokens/refusal；`pause_turn`（长时服务器工具轮次暂停）与 `model_context_window_exceeded` 落 default→"stop"。LiteLLM 同样未映射（default→stop+warning）
- 与 LiteLLM 行为一致，可接受；建议：`model_context_window_exceeded` 映射为 "length" 更贴语义；`pause_turn` 至少加显式分支与日志（客户端收到 "stop" 但对话未真正终结）

### A6【低】usage：`cache_tokens = cache_read + cache_creation`，与 OpenAI `cached_tokens`（命中语义）有偏差；且与 Gemini 路径的对外暴露不一致

- 位置：`anthropic.rs:345-362`（extract_usage）；对外 JSON：`anthropic.rs:456` 用 `client_usage_json`（无 prompt_tokens_details），`gemini.rs:515` 用 `cached_client_usage_json`
- 官方语义：`prompt_tokens_details.cached_tokens` = 缓存命中（read）；Anthropic 的 `cache_creation_input_tokens` 是缓存写入
- LiteLLM：`prompt_tokens = input + read + creation`（与本地一致，正确）；但 `cached_tokens = cache_read_input_tokens`，creation 单独放 `cache_creation_tokens`
- 影响：网关数据面板「缓存命中率」把首次写入也算成命中，Anthropic 流量命中率系统性偏高；同时 Anthropic 路径从不向客户端暴露 cached_tokens 明细而 Gemini 暴露，跨协议不一致
- 建议：`cache_tokens` 只算 read；creation 如需保留进单独指标字段

### A7【低】`tool_choice:{type:"none"}` 上附加 `disable_parallel_tool_use` 不符合官方 schema

- 位置：`anthropic.rs:240-245`（`map_tool_choice` 对任意 base 插入 `disable_parallel_tool_use`）
- 官方：`ToolChoiceNone` 只有 `type` 字段（无 `disable_parallel_tool_use`）；auto/any/tool 才有该字段
- 对照：LiteLLM 在 `tool_choice=="none"` 时显式跳过该 flag
- 影响：`parallel_tool_calls:false + tool_choice:"none"` 组合发出含未知字段的 none 对象（视上游校验严格度可能 400）
- 建议：none 时不加该 flag

### 确认正确（Anthropic）

- system/developer → 顶层 `system` text blocks 数组、多条合并；连续同 role 合并（官方本就会合并 consecutive turns，无害且更稳）
- `role:"tool"` → user 消息内 `tool_result`；assistant `tool_calls` → `tool_use`（arguments 解析失败回退 `{}`）
- tool_choice 映射表与官方 ToolChoice 四形态一致（auto/any/tool/none；缺省 parallel=true 不发 flag，正确）
- stop → stop_sequences（过滤空白项）
- max_tokens 缺省 4096（官方必填；LiteLLM 回退值同为 4096）
- 响应侧：text/thinking/tool_use → content/reasoning_content/tool_calls；refusal→content_filter；`tool_use` 优先 tool_calls（正确）
- 流式：message_start 带输入 usage、message_delta 合并输出 usage 并发 finish chunk、thinking_delta→reasoning_content、input_json_delta 增量拼接、json 模式缓冲到 message_delta 统一解包输出——事件模型与官方 SSE 序列吻合；官方确认 message_delta 的 usage 是累计值且可含 input_tokens/cache_* 字段，本地 `extracted.x.or(previous.x)` 合并逻辑正确处理
- usage 口径：官方原文 "Total input tokens in a request is the summation of `input_tokens`, `cache_creation_input_tokens`, and `cache_read_input_tokens`"——本地 prompt_tokens = input+read+creation 正确
- assistant 的 reasoning_content 不回传是正确取舍（官方要求 thinking 块必须带原样 signature 回传且不得改动，网关未保存 signature，无法安全回传）
- 备注：Anthropic image `url` source 在直连 API 可用，但官方注明 "On Amazon Bedrock and Google Cloud, only base64-encoded sources are currently available"——若上游是 Bedrock/GCP 兼容端点，http(s) 图片 URL 也会失效

---

## 二、Gemini 方向（src/proxy/convert/gemini.rs）

### B1【高】http(s) 图片 URL 转 `fileData{fileUri}`，官方不接受，上游 400

- 位置：`gemini.rs:352-362`（`image_part`：非 data: URL → `{"fileData":{"fileUri":url}}`）
- 官方：`FileData.file_uri` = "The URI of the file in **Google Cloud Storage**"（proto/SDK 原文）；AI Studio API 仅接受 Files API / GCS URI（及 YouTube 等），任意 https 图片 URL 非法
- LiteLLM：AI Studio 路径注释明说 "Google AI Studio Gemini does not support HTTP/HTTPS URLs for files. Convert them to base64 data instead."——一律下载转 `inline_data` base64
- 影响：OpenAI 客户端发图片 URL + Gemini 成员 → 上游 INVALID_ARGUMENT；failover 后可能落到其他成员掩盖问题
- 建议：下载图片转 inlineData（与 LiteLLM 对齐）；至少对非 GCS/Files URI 不发 fileData

### B2【中】thoughtSignature 未保存未回传，Gemini 2.5/3 thinking + function calling 多轮会劣化或 400

- 位置：`gemini.rs:419-452`（`parts_to_message` 丢弃 functionCall part 上的 `thoughtSignature`）；`gemini.rs:67-71`（assistant reasoning 回传 `{text,thought:true}` 不带签名）
- 官方：`Part.thought_signature`（bytes/base64）"An opaque signature for the thought **so it can be reused in subsequent requests**"
- LiteLLM：tool_call part 携带 thought_signature 回传，gemini-3 无真实签名时注入 dummy 签名（"Function calling with thinking" 强依赖）
- 影响：Gemini thinking 模型多轮工具调用在第 2 轮以后可能报错或丢失推理上下文
- 建议：流式/非流式转换时把 functionCall part 的 thoughtSignature 编进 tool_call（如 LiteLLM 藏进 id 或 provider_specific 字段），下一轮请求时还原

### B3【低】presencePenalty / frequencyPenalty 未映射

- 位置：`gemini.rs:128-150`（generationConfig 只映射 maxOutputTokens/temperature/topP/seed/stopSequences/响应格式）
- 官方：GenerationConfig 含 `presencePenalty`、`frequencyPenalty`（JSON representation 已确认）
- LiteLLM：同名透传（非 Gemini-3 模型）
- 影响：带惩罚参数的请求对 Gemini 成员静默丢失参数

### B4【中】promptFeedback.blockReason 仅 debug 日志，客户端拿到空内容 + finish "stop"

- 位置：`gemini.rs:473-478`（只 `tracing::debug!`）；`gemini.rs:499-503`（无 candidate → finish_reason 默认 "stop"）
- 官方：`PromptFeedback.blockReason`（SAFETY/OTHER/BLOCKLIST/PROHIBITED_CONTENT/IMAGE_SAFETY）表示 prompt 被拦、无候选
- LiteLLM：返回 `finish_reason="content_filter"` + `content:null`（`_handle_blocked_response`）
- 影响：内容被拦截时客户端误以为模型正常返回了空回答，重试也无意义；语义上应为 content_filter（或错误）
- 建议：blockReason 存在时 finish_reason → "content_filter"

### B5【低】tool 结果非 object 的 JSON（数组/数字/null）会原样塞进 functionResponse.response → 400

- 位置：`gemini.rs:101-103`（`serde_json::from_str` 成功即直接用作 `response`）
- 官方：`FunctionResponse.response` REQUIRED、`google.protobuf.Struct`（"The function response in **JSON object format**"）
- LiteLLM：数组/非 dict 解析后包成 `{"content": ...}`
- 影响：工具返回 `"null"`/`"[1,2]"`/`"123"` 等合法 JSON 但非 object 时上游 400；普通字符串已被 `{"result":raw}` 包住不受影响
- 建议：解析结果非 object 时包一层（如 `{"result": parsed}`）

### B6【低】json_schema 走 `responseJsonSchema` 的兼容性说明

- 位置：`gemini.rs:160-168`
- 官方：v1beta GenerationConfig 同时存在 `responseSchema`（OpenAPI 子集）与 `responseJsonSchema`（完整 JSON Schema，本次已在 JSON representation 与 genai SDK（`response_json_schema`）确认存在）——本地选择合法且更新
- 风险：第三方「Gemini 兼容」上游可能只实现 `responseSchema`，`responseJsonSchema` 被忽略导致 JSON 约束失效；且本地对 responseJsonSchema 仍做 sanitize（内联 $ref、删键），对支持完整 JSON Schema 的字段是多余信息损失
- 建议：保持现状可接受；如需兼容旧上游可按模型回退 responseSchema

### B7【低】finishReason 映射小注

- 位置：`gemini.rs:365-391`
- 与官方枚举对照：`MALFORMED_FUNCTION_CALL`/`TOO_MANY_TOOL_CALLS`→stop、`IMAGE_SAFETY`/`IMAGE_PROHIBITED_CONTENT`→content_filter 均与 LiteLLM 一致；`MALFORMED_RESPONSE` 不在官方 v1beta FinishReason 枚举中（多余项，无害）；`UNEXPECTED_TOOL_CALL`/`IMAGE_OTHER`/`NO_IMAGE`/`IMAGE_RECITATION` 未列出 → 落 default "stop"（LiteLLM 同样落 stop），可接受

### B8【低】流式 chunk 的 model 字段死代码

- 位置：`gemini.rs:577-579`（`self.model` 从 modelVersion 赋值但 chunk 全用 `requested_model`）；`gemini.rs:639-651`（final_chunk 的 ensure_started 写入被丢弃的 Vec，纯空流时客户端收到无 role delta 的 finish chunk）

### 确认正确（Gemini）

- contents role user/model、assistant→model、system→systemInstruction（`{parts}` 形状合法）、tool→`functionResponse{name,response}`（role 包在 user 中，与 LiteLLM/官方一致）
- tool_choice→functionCallingConfig（AUTO/ANY/NONE + allowedFunctionNames 仅 ANY，正确）
- schema 清洗：type 大写、format 白名单（STRING: enum/date-time；NUMBER/INTEGER: float/double/int32/int64）与官方 Schema 允许集一致；$ref 内联、类型数组取非 null + nullable、空 properties 删除
- usage：output = candidates + thoughts（官方 total = prompt + thoughts + candidates，等价于兜底分支 total−prompt）、cache = cachedContentTokenCount、prompt 含缓存——与 OpenAI 语义及 LiteLLM 一致
- assistant reasoning_content → `{text,thought:true}` 回传与 LiteLLM 同款（官方 Part.thought 为合法请求字段）
- 流式：每 chunk 即完整 GenerateContentResponse、finishReason/usageMetadata 逐 chunk 覆盖、流末由管线补 finish chunk + [DONE]——与官方 streamGenerateContent 语义吻合

---

## 三、Responses 方向（src/proxy/convert/responses.rs）

> 官方文档 agent 因并发限制未产出，本节以 LiteLLM 桥接实现与已知官方规格交叉印证为主；C1 为高置信度问题（LiteLLM 对照明确）。

### C1【高】响应以 function_call 收尾时 finish_reason 恒为 "stop"，客户端无法感知需要执行工具

- 位置：`responses.rs:191-201`（`finish_from_status`：completed→"stop"）+ `responses.rs:489-512`（response.completed 直接用 status 定 finish_reason）
- 对照：LiteLLM 桥在 `response.completed` 时**检查 `response.output` 中是否有 function_call** → finish_reason = "tool_calls" / "stop"（transformation.py:1512-1549）；OpenAI finish_reason 枚举中 `tool_calls` 的语义即「等待客户端执行工具」
- 本地现状：工具调用参数会正常流式/聚合输出（`emit_final_output` 兜底完整），但 finish chunk 恒为 "stop"（现有测试 `converts_stream_events` 恰好固化了这一错误行为）
- 影响：所有依赖 finish_reason=="tool_calls" 的 OpenAI 生态客户端（openai-python agents、LangChain 等）会把工具调用轮误判为最终回答，工具链路断裂——这是四协议里对客户端语义破坏最大的一条
- 建议：completed/incomplete 收尾时扫描 `response.output` 是否含 `function_call` 项，有则 finish_reason="tool_calls"

### C2【中·管线】Responses 出站强制流式，但对流式客户端也是全量缓冲后再发

- 位置：`src/proxy/mod.rs:1434-1444`（`collect_stream_events(...).await` 聚合完整个上游流后才进入 client_stream 分支）
- 后果：流式客户端的首字节延迟 = 上游完整生成时长（「流式」名存实亡），且整个响应驻留内存；非流式客户端的聚合行为正常
- 建议：流式客户端直接边转换边转发（Anthropic/Gemini 路径已是如此，模式可复用），非流式客户端保留聚合

### C3【低】无 system 消息时注入默认 instructions "You are a helpful assistant."

- 位置：`responses.rs:73-84`
- 对照：官方 instructions 可选；LiteLLM 仅在有 system 消息时设置（`if instructions:`）
- 影响：客户端未写 system 时向上游注入了客户端不知情的指令，改变模型行为
- 建议：instructions 为空时不设置

### C4【低】max_output_tokens 未按官方最小值 16 钳制

- 位置：`responses.rs:87-89`；LiteLLM 把 <16 抬到 16（openai responses transformation）。max_tokens=10 → 上游 400

### C5【低】tools / json_schema 的 strict 未透传

- 位置：`responses.rs:102-120`（function tool 丢弃 `strict`）、`responses.rs:146-157`（text.format json_schema 丢弃 `strict`）
- 对照：LiteLLM 原样透传 chat 侧 strict。影响 schema 校验行为与上游默认值不一致

### 确认正确（Responses）

- 请求：input item 映射（user→input_text、assistant→output_text、tool_calls→function_call{call_id,name,arguments}、tool→function_call_output）与官方 item 类型一致；store:false、强制 stream:true 为合理设计取舍；temperature/top_p/tool_choice/parallel_tool_calls 透传；chat 专有参数（stop/seed/penalties）正确丢弃（Responses 不支持）
- 流式：事件名与官方一致（response.created/in_progress/output_text.delta/reasoning_summary_text.delta/output_item.added/output_item.done/function_call_arguments.delta/completed/incomplete/failed/error）；额外兼容非官方的 `reasoning_text.delta`（vLLM/gpt-oss 类后端）；output_item.done + response.completed 双重兜底「缺失后缀」设计比 LiteLLM 更稳健
- tool_call 的 output_index→连续 index 重映射与 LiteLLM 同思路（Responses 的 output_index 含 reasoning/message 项，必须重映射）
- usage：input_tokens→prompt_tokens、cached_tokens 取 input_tokens_details、total 补算——与 LiteLLM 一致

---

## 四、管线级（src/proxy/mod.rs、convert/mod.rs）

### D1【低】OpenAI 直通注入 `stream_options.include_usage`，未请求的客户端也会收到 usage chunk

- 位置：`convert/openai.rs:10-37`（注入）+ `mod.rs:1384-1432`（字节直通不剥离）
- 官方：usage chunk 仅在 include_usage=true 时出现；其余 chunk `usage:null`
- 对照：LiteLLM 默认不注入（`always_include_stream_usage` 显式开启才注入，且对调用方剥离）
- 影响：多数客户端容忍；严格按规范实现的客户端可能异常。属为指标做的取舍，建议改为「注入但不透传给客户端」

### D2【低】响应 `model` 字段跨协议不一致

- Anthropic/Gemini → 虚拟模型名；Responses → 上游真实模型（response.created 覆盖）；OpenAI 直通 → 上游真实模型。建议统一为虚拟模型名（与请求的 model 呼应）

### D3【nit】`chunk_json` 的 `created` 每次取当前时间

- 位置：`convert/mod.rs:99-111`。官方要求同一 completion 的所有 chunk 共享同一 `created`；跨秒时同一流内 created 会漂移。构造 converter 时生成一次即可

### D4【nit】全局限制备忘

- 多模态仅支持 image_url；`input_audio`/`file` 部件在四个协议请求侧均被静默丢弃
- `n>1` 未支持（转换协议只取 candidates[0]；OpenAI 直通则原样多 choice）
- 转换协议的响应 message 无 `refusal` 字段（官方 schema 中为 required，主流 SDK 容忍缺省）
- usage 的 reasoning_tokens（OpenAI completion_tokens_details / Anthropic thinking tokens / Gemini thoughtsTokenCount）未进 request 表指标

---

## 五、修复优先级建议

1. **C1**（Responses tool_calls finish_reason）——破坏 OpenAI 客户端工具循环语义，影响面最大且修复局部
2. **A1**（minimal→128 必 400）+ **A3/A4**（thinking 组合 400）——一批处理：`map_thinking` 收口钳制与互斥
3. **B1**（Gemini 图片 URL 下载转 base64）
4. **B4**（Gemini blockReason → content_filter）
5. **A2**（adaptive thinking / output_config.effort）——需要模型代际信息，单独立项
6. **B2**（thoughtSignature 透传）——涉及跨轮状态编码，单独立项
7. C2（Responses 流式直通）、C3/C4/C5、A6/B3 及各 nit 批量收尾
