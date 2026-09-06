# OpenRouter 协议转换对照报告

- 日期：2026-09-06
- 范围：与 `.scratch/protocol-conversion-audit/FINDINGS.md`（官方文档 + LiteLLM 对照审计）对账，本轮改以 OpenRouter 公开行为为参照
- 方法：官方一手来源（OpenRouter 文档仓库 [OpenRouterTeam/docs](https://github.com/OpenRouterTeam/docs) 的 mdx 源文件 + 官方 `openapi.yaml`）逐专题调研，再与本地 `src/proxy/convert/` 四协议实现逐点比对
- 与 FINDINGS 的编号关系：本轮发现用 **O 系列**；与 FINDINGS A/B/C/D 条目的关联在各条目标注

## 一、OpenRouter 机制要点（调研结论）

### 入口形态

- 对外统一 OpenAI Chat Completions（`/api/v1/chat/completions`），另有两套「协议皮肤」：Anthropic Messages 兼容（`/api/v1/messages`）与 OpenAI Responses 兼容（`/api/v1/responses`，已 GA）。
- 内部三层：上游 Adapter（各家 SSE/JSON → 统一内部事件）→ Plugin 链 → Skin Transform（按客户端请求的皮肤输出）——转换双向发生。
- 管理端点齐全（/models、/key、/generation 等）；`GET /models` 每条模型元数据含 `reasoning` 对象（`supported_efforts`/`default_effort`/`default_enabled`/`supports_max_tokens`/`mandatory`）。

### 思考块处理（重点）

- 请求侧主参数是 `reasoning` 对象：`{effort, max_tokens, exclude, enabled, context, mode, summary}`；`reasoning_effort` 仅是 `reasoning.effort` 的 shorthand（openapi 注释原文："Equivalent to setting reasoning.effort"）；`include_reasoning` 是 legacy。
- 响应侧非流式 `message.reasoning`（字符串）+ `message.reasoning_details`（数组）；**`reasoning_content` 是 `reasoning` 的别名**（文档原文）。流式走 `delta.reasoning` / `delta.reasoning_details`。
- `reasoning_details` 四种 type：`reasoning.text`（text + signature）、`reasoning.encrypted`（data 必填）、`reasoning.summary`、`reasoning.server_tool_call`（新）；公共字段 `id`/`format`/`index`。`format` 枚举：`unknown`、`openai-responses-v1`、`azure-openai-responses-v1`、`bedrock-openai-responses-v1`、`bedrock-xai-responses-v1`、`xai-responses-v1`、`meta-responses-v1`、`anthropic-claude-v1`（默认）、`google-gemini-v1`。回传要求**原样、不可重排或修改**。
- effort→预算换算：OpenAI/Grok 只吃 effort；Anthropic budget 模式（4.5 及更早）`budget_tokens = clamp(max_tokens × ratio, 1024, 128000)`，ratio：max/xhigh=0.95、high=0.8、medium=0.5、low=0.2、minimal=0.1；**Claude 4.6+ 改 `thinking:{type:"adaptive"}` + `output_config.effort`**（effort 直接映射、minimal→low、none 不发；与 `verbosity` 同一上游字段）；Gemini 3 effort 直接映射 `thinkingLevel`（`reasoning.max_tokens` 才走 thinkingBudget），Gemini 2.5 走旧 thinkingBudget。
- Anthropic 摘要思考：OpenRouter 对支持 `thinking.display` 的 Claude 默认 `display:'summarized'`。

### 流式与错误

- usage 恒返回（`stream_options.include_usage` 已废弃无效果）；usage chunk 的 **choices 非空**，重复最终 finish_reason 并附 `native_finish_reason`；SSE 注释行 `: OPENROUTER PROCESSING` 作 keep-alive。
- finish_reason 归一 5 值：`tool_calls`/`stop`/`length`/`content_filter`/`error`，原始值透传 `native_finish_reason`（Anthropic/Gemini 全枚举逐条对照表官方未公布）。
- 中途错误：顶层 `error` 对象（含 `metadata.error_type` 统一错误分类）+ `finish_reason:"error"` 终止 chunk，HTTP 仍 200。
- usage 结构：`prompt_tokens_details.cached_tokens`（读缓存）+ `cache_write_tokens`（写缓存单列）；`completion_tokens_details.reasoning_tokens`；另有 cost/cost_details。

### Responses 上游

- 内部确认存在「Responses API adapter family」（内部架构文档 STREAMING.md），o 系/gpt-5 走原生 Responses 接入；对外 Responses 入口强制 stateless（`store` 只接受 false、`previous_response_id` 非空 400），reasoning item 原生支持 `encrypted_content`，回传配合 `include:["reasoning.encrypted_content"]`。

## 二、总体结论

本地转换实现与 OpenRouter 公开语义**没有方向性错误**；`reasoning_details` 无损透传（3ffa3c3）的格式逐字段与官方 OpenAPI 吻合，三条注入链路语义正确。差距集中在**遗漏**（OpenRouter 有而本地没有），最大一项是 O1。

## 三、发现清单

### O1【遗漏·中】请求侧不认 `reasoning` 对象，OpenRouter 风格客户端思考静默失效

- 本地：三个转换协议的思考入口都只读 `chat.get("reasoning_effort")`（`src/proxy/convert/anthropic.rs:284`、`src/proxy/convert/gemini.rs:314`、`src/proxy/convert/responses.rs:102`）。
- OpenRouter：`reasoning` 对象是主参数形态；`reasoning_effort` 只是 shorthand。
- 影响：客户端按 OpenRouter 主流写法发 `"reasoning":{"effort":"high"}`（或 `{max_tokens}`/`{enabled}`）时，网关完全不识别——Anthropic 成员不发 thinking、Gemini 不发 thinkingConfig、Responses 不发 reasoning，**请求成功但不思考**，用户难以归因。
- 建议：最低成本支持 `reasoning.effort`（与现有 reasoning_effort 归一）；有余力再支持 `max_tokens`（Anthropic 直传 budget）与 `exclude`。

### O2【差异·中】effort→budget 换算口径不同

- 本地：固定档位（`src/proxy/convert/mod.rs:103`，LiteLLM 风格 low=1024/medium=2048/high=4096/xhigh=8192/max=16384，再 `min(max_tokens-1)`；A1 的 minimal 钳 1024 已修）。
- OpenRouter：按 max_tokens 比例换算（ratio 0.95/0.8/0.5/0.2/0.1），clamp [1024, 128000]。
- 影响：两者都合法，但大 max_tokens 场景本地思考预算明显偏小（如 max_tokens=64000 + high：OpenRouter 51200 vs 本地 4096）；本地 xhigh/max 档位与 OpenRouter 比例语义无对应关系。
- 建议：口径决策；若要贴 OpenRouter 生态行为改比例换算。

### O3【跟进·中】Gemini 3 应映射 `thinkingLevel`（本地恒用 `thinkingBudget`）

- 本地：`src/proxy/convert/gemini.rs:319-322` 恒发 `thinkingConfig.thinkingBudget + includeThoughts`。
- OpenRouter：Gemini 3 effort 直接映射 `thinkingLevel`；thinkingBudget 仅作 `reasoning.max_tokens` 直传通道；Gemini 2.5 走旧 thinkingBudget。
- 影响：Gemini 2.5 正确；Gemini 3 上 thinkingBudget 仍被接受但语义被 thinkingLevel 覆盖。
- 建议：与模型代际信息一并处理（与 O4/O2 同捆）。

### O4【遗留印证·中】A2 adaptive thinking——OpenRouter 提供现成参照

- OpenRouter 已实施 Claude 4.6+ 的 `thinking:{type:"adaptive"}` + `output_config.effort` 映射（minimal→low、none 不发 effort），`verbosity` 参数亦映射同一字段。
- 结论：印证 FINDINGS A2 的整改方向是业界实际做法，可与 O3/O2 捆绑为「模型代际感知」小立项。

### O5【错误·低】Anthropic 非流式 usage 缺 `prompt_tokens_details.cached_tokens`，跨协议不一致

- 本地：`src/proxy/convert/anthropic.rs:562` 用 `client_usage_json`（仅三项）；Gemini（`src/proxy/convert/gemini.rs:736`）、Responses 聚合（`src/proxy/mod.rs:943` `accumulate_chunks`）与全部流式路径用 `cached_client_usage_json`（带缓存命中明细）。
- 影响：Anthropic 恰是缓存语义最重的协议，明细反而缺失。一行改动对齐。
- 关联：A6 的 `cache_tokens = read + creation` 口径——OpenRouter 明确 `cached_tokens`=读缓存、写缓存单列 `cache_write_tokens`，**印证**本地把 cache_creation 计入命中率会系统性虚高。

### O6【差异·低，可选增强】

| # | 点 | OpenRouter | 本地 | 评估 |
| --- | --- | --- | --- | --- |
| O6a | `native_finish_reason` 透传 | 归一 5 值 + 附原生值 | 归一 4 值（无 error），原生信息丢失（`pause_turn` 折成 stop 后不可分辨） | 可选增强；与 A5 相关 |
| O6b | 思考键名 | 主键 `reasoning`（`reasoning_content` 仅别名） | 转换链路输出 `reasoning_content` | 已拍板前端双识别（7566c17），保持现状合理；贴 OpenRouter 生态可双键输出，成本极低 |
| O6c | 流式 usage 策略 | 恒发 usage（include_usage 废弃），usage chunk choices 非空 | 严格按 OpenAI 规范按需发送（include_usage 才发，choices 空数组） | 规范 vs 生态取舍，保持现状合理；OpenRouter 迁移客户需显式设 include_usage |
| O6d | 流式错误终止形态 | 顶层 error 帧 + `finish_reason:"error"` 终止 chunk | 顶层 error 帧（`src/proxy/mod.rs:1832-1840`）后直接 `[DONE]` 无 finish_reason | 主流客户端认顶层 error，语义等价，可接受 |
| O6e | 扩展采样参数 | 支持 `top_k`/`min_p`/`top_a`/`repetition_penalty` | 全忽略 | `top_k` 对 Anthropic/Gemini 有原生意义，可低成本支持 |
| O6f | 多模态面 | `input_audio`/`file`(PDF)/`video_url` 完整支持（含自研 PDF 解析） | 静默丢弃（= FINDINGS D4） | 差距大但非近期刚需 |
| O6g | 错误归一 | 统一 `error_type` 分类体系（三皮肤通用） | 透传上游原始消息 | 可借鉴 |
| O6h | 杂项 | 参数缺失不注入默认值 | Anthropic max_tokens 必填缺省 4096（偏小）；消息 `name` 前缀化缺失；SSE keep-alive 注释帧不发 | 无关紧要；max_tokens 缺省可考虑抬高 |

## 四、对账表（与 FINDINGS 关系）

| 本轮条目 | FINDINGS 关联 | 说明 |
| --- | --- | --- |
| O1 | 无重叠 | 新发现（OpenRouter 生态参数面） |
| O2 | A1 已修部分重叠 | A1 修的是 minimal 钳制；换算口径是新维度 |
| O3 | 无重叠（B 系列未涉及） | 新发现 |
| O4 | **A2 印证** | OpenRouter 实操验证 A2 整改方向 |
| O5 | **A6 相关 + 新不一致点** | cached_tokens 语义印证 A6；Anthropic 非流式缺明细是新发现 |
| O6a | A5 相关 | native 值透传可一并解决 A5 的语义丢失 |
| O6b | 无重叠 | 新（与直通链路键名拍板 7566c17 不冲突，仅转换链路） |
| O6c | D1 相关 | D1 已修为「注入但过滤」；与 OpenRouter 恒发的策略差异是有意取舍 |
| O6f | **D4 印证** | 同一条遗留 |
| — | **B2 状态修正** | FINDINGS 整改段仍标 B2 未修复，实际 thoughtSignature 透传已随 3ffa3c3 实现（`gemini.rs:232-249,696-710,832-858`），文档滞后 |

## 五、确认与 OpenRouter 一致（抽查通过）

- **reasoning_details 格式**：`reasoning_text_detail`/`reasoning_encrypted_detail`（`src/proxy/convert/mod.rs:23-53`）字段结构与官方 OpenAPI 逐字段吻合；format 三个常量（`anthropic-claude-v1`/`openai-responses-v1`/`google-gemini-v1`）均在官方枚举内；请求侧校验保持原序（签名链不重排）；按 format 匹配注入、不匹配丢弃（OpenRouter 未披露跨厂商行为，本地设计合理）。
- **三条注入链路**：Anthropic thinking/redacted_thinking（签名空跳过、仅 thinking 启用时注入、tool_calls 轮无块丢弃 thinking 参数）；Responses reasoning item（encrypted_content + 恒带 `include:["reasoning.encrypted_content"]`，与官方回传模式一致）；Gemini thoughtSignature 按 tool_calls 下标回挂 functionCall part。
- **其余抽查**：finish_reason 映射表与官方枚举一致；SSE 解析容忍注释行/多 data 行/CRLF（上游发 keep-alive 注释不会炸）；tool_calls 流式增量；tool_choice 映射（含 thinking 互斥降级）；json_schema 清洗；usage 聚合口径（Responses 非流式含缓存明细）。

## 六、建议处理顺序

1. **O1**（reasoning 对象识别）——真实用户可感知的功能缺失，改动局部（三处入口 + 一个归一函数）。
2. **O5**（Anthropic 非流式 usage 明细对齐）——一行改动；顺手拍板 A6 口径。
3. **O3+O4+O2**（Gemini 3 thinkingLevel / Claude adaptive / 比例换算）——同捆「模型代际感知」小立项，OpenRouter 行为可直接作参照。
4. O6 各项按需取用（O6a native_finish_reason 与 O6e top_k 性价比最高）。

入口形态差异（OpenRouter 三皮肤 vs 本地单端点）属产品定位差异不算遗漏；`/api/v1/messages` Anthropic 兼容入口可让 Claude Code 等原生 Anthropic 协议客户端直连网关，列为路线图参考项。
