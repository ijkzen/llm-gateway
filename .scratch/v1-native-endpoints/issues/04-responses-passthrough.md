# 04: /v1/responses 原生透传端点

**What to build:** OpenAI 原生 Responses agent（如 Codex）可指向 /v1/responses 使用 Responses 类型虚拟模型：复用工单 03 的透传管线，请求/响应体原样透传（仅改写 model 字段），流式与非流式均支持。Bearer 鉴权。错误响应用 OpenAI error 原生格式。usage 从 Responses 原生响应解析归一落现有 request 表字段（cached_tokens→input_cache_tokens，reasoning tokens 计入 output_tokens）；实现时验证流式 response.completed 与非流式响应体的 usage 可用性，确需 opt-in 才做最小注入并在本工单记录。

**Blocked by:** 03（透传管线与 /v1/messages 端点）。

**Status:** ready-for-agent

- [ ] POST /v1/responses 只接受 Responses(1) 类型虚拟模型，其他类型按 OpenAI 错误格式拒绝
- [ ] 请求体仅改写 model；响应（流式/非流式）原样中继
- [ ] Responses usage 解析（response.completed / 非流式响应体）落 request 表（缝 C 纯函数单测）
- [x] usage 可用性验证结论记录在工单内（是否需要 opt-in 注入）

## Comments

### 2026-09-07 实现结论（usage 可用性验证）

- Responses API 的 usage 无需 opt-in：非流式响应体与流式 `response.completed` /
  `response.incomplete` / `response.failed` 事件的 `response.usage` 恒携带用量
  （input_tokens / output_tokens / input_tokens_details.cached_tokens /
  output_tokens_details.reasoning_tokens），网关按旁路扫描解析，不做请求侧注入。
- Anthropic Messages 同理：`message_start`（输入侧，含 cache_read/cache_creation）⊕
  `message_delta`（output_tokens）合并解析，无 opt-in 参数。
- 口径备注：`input_cache_tokens` 沿用线上既有口径——只记 cache_read（cache_creation
  是写入不计命中，OpenRouter 同口径，见 convert/anthropic.rs extract_usage 注释）；
  spec 决策 10 原文「cache_creation+cache_read→input_cache_tokens」为行文误差，已修正。
- [ ] 集成测试：mock Responses 上游验证透传、类型路由、usage 落库（缝 A）
