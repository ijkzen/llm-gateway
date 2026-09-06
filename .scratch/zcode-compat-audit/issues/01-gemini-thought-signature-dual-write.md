# 01: Gemini thoughtSignature 双写闭环（ZCode/AI SDK 兼容）

**What to build:** 客户端是 ZCode（或其他基于 Vercel AI SDK 的客户端）时，经网关访问 Gemini 3 系上游的工具调用对话在第二轮起不再 400。具体做法：网关把 Gemini 上游返回的 `thoughtSignature` 除了继续写进 OpenRouter 风格的 `reasoning_details`（保留现有行为，不破坏其他客户端）之外，**双写**一份到对应 `tool_calls[i].extra_content.google.thought_signature`（流式 delta 与非流式 message 两条路径都要）；请求侧 Gemini 注入签名时，除了现有的 `reasoning_details`（format=google-gemini-v1）来源，也认 assistant 消息里 `tool_calls[].extra_content.google.thought_signature` 这一来源。背景与数据流分析见 `../FINDINGS.md` P1。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 流式与非流式响应中，带签名的 tool_call 同时出现在 reasoning_details 和 extra_content.google.thought_signature 两处
- [ ] 请求侧仅从 extra_content.google.thought_signature（无 reasoning_details）也能把签名按 tool_calls 下标挂回对应 functionCall part
- [ ] 双来源同时存在时不重复注入、不冲突
- [ ] 端到端验证：ZCode→网关→Gemini 3 上游的多轮工具对话第二轮不再 400
- [ ] cargo fmt / clippy -D warnings / cargo test 全绿
