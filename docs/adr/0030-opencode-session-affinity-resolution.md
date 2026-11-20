# 0030 — OpenCode 会话头按客户端原生信号分层解析

## Status

accepted

## Context

OpenCode Go 要求每个推理请求携带 `x-opencode-session`，否则部分上游返回 `400 MissingSessionID`。该头不是鉴权凭据，而是**会话亲和提示**：任意稳定唯一字符串即可，OpenCode 借此把同一对话的多轮请求固定到同一后端，复用 prompt cache（官方说明缓存命中率可从近乎 0 提升到 90%+）。

网关原实现（`0f420f9`）按「API Key + 本地日期」派生 UUIDv5 回退值。这只能满足存在性检查，无法识别真实会话：同一个 Key 当天所有对话（包括并发的无关对话）被折叠成同一个会话，不同 Key 的同一对话也无法关联。OpenCode Go 侧因此无法正确固定路由与缓存。

与此同时，主流 coding agent 已经在请求中携带真实的 per-conversation 标识：

- Claude Code：`X-Claude-Code-Session-Id` 头，以及 body `metadata.user_id`（JSON 字符串内的 `session_id` 或旧版 `..._session_<uuid>` 整串）。
- Codex CLI：`session-id` / `session_id` 头，以及 body `prompt_cache_key`。
- OpenCode / pi / Crush：`X-Session-Id` / `X-Session-Affinity` 头；pi / DeepSeek Harness 另有 `n-session`。
- ZCode：Go 端识别其原生头，但头名未公开。

Go 端对部分客户端的原生头已能自行识别，但网关在同一条链路上仍会注入天级回退值，二者口径不一致。需要一个统一、可配置、无状态的解析规则。

## Decision

1. **分层优先级链**：新增 `resolve_opencode_session(downstream: &HeaderMap, body: &Value, api_key_name: &str, native_header_names: &[HeaderName]) -> String`，按以下顺序取第一个有效值：
   1. 客户端显式 `x-opencode-session`（HeaderMap 原值，大小写不敏感）；
   2. 原生会话头，顺序由设置项 `opencode_session_header_names` 的数组顺序决定，默认为 `x-claude-code-session-id` → `session-id` → `session_id` → `x-session-id` → `x-session-affinity` → `n-session`；
   3. body 字段：`metadata.user_id`（JSON 对象/JSON 字符串取 `session_id`，否则取整串）→ `prompt_cache_key` → `session_id`；
   4. 首条 user message 文本（string 或 content part 数组，截断 4096 字节）按 API Key + 文本派生 UUIDv5，前缀 `auto-`；
   5. 原有按 API Key + 本地日期（设置表时区，缺省 Asia/Shanghai）的 UUIDv5 回退。

   所有候选值需 trim、非空且能通过 `HeaderValue::from_str`（拒绝 CRLF/非 ASCII），无效值跳到下一级，保证绝不会因脏值缺头。

2. **入口解析一次、全程复用**：`/v1/chat/completions` 在路由 handler（持有原始 `HeaderMap` 与已解析 `Json<Value>`）解析；`/v1/messages` 与 `/v1/responses` 在 `forward_native` 内（body 解析后）解析。解析结果作为 `String` 传入 `forward_chat` / `forward_native`，再经既有的 `forward_through_members` → `ForwardFlavor::build` → `build_*_upstream_call` → `assemble_outbound_headers` 链路贯穿所有 failover 尝试，不在中途重算。管理后台聊天、测速探测、模型探测入口无下游头，维持原回退值。

3. **注入门禁不变**：只有上游 host 为 `opencode.ai`、且出站表中尚无 `x-opencode-session` 时才写入。下游 allowlist 透传值与 provider `custom_header` 仍优先于解析值——显式配置永远压过自动识别。

4. **可配置头名**：新增 Json 类型设置 `opencode_session_header_names`（种子默认值即上述 6 个头名），复用 `downstream_request_header_allow_list` 的解析、缓存、热更新与黑名单校验管线（`PUT /api/settings/{key}` 即时生效，删除后恢复默认）。用于补充 ZCode 等未公开头名，并允许按部署实际调整优先级。

## Consequences

- 同一 Key 下的并发对话按原生会话 ID / 首条消息内容区分，OpenCode Go 能稳定固定路由与 prompt cache；不再出现「当天所有请求共用一个会话」的退化。
- 客户端未携带任何会话信号时退化为内容指纹，仍优于按天折叠；单轮无上下文的请求才会落到最终的天级回退。
- 会话值中可能包含客户端生成的会话 ID（UUID 等），这些值会作为出站头发往 OpenCode 上游；这是会话亲和机制的必要前提，且只在 `opencode.ai` 上游发生。
- 原生会话头本身**不**因本决策进入 chat/completions 的 allowlist（避免额外数据出站）；原生透传端点仍按既有黑名单策略全量透传。
- 头名列表与 allowlist 共用校验：黑名单头（凭据/框架/hop-by-hop）无法写入，读侧再兜底过滤。
- 测试：单元测试覆盖每级优先级、头名大小写、`metadata.user_id` 两种形态、无效值跳过、内容指纹稳定性、解析→注入组合；集成测试覆盖设置热更新与校验；回归覆盖既有透传优先级与非 opencode 上游不注入。
