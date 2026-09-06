# 07: `native_finish_reason` 原生终止值透传（O6a，关联 A5）

**Parent**: `.scratch/protocol-conversion-audit/OPENROUTER-COMPARISON.md` O6a、FINDINGS A5

**What to build:** 归一 finish_reason 的同时，在响应 choice 上附 `native_finish_reason` 字段透传上游原生终止值（Anthropic stop_reason / Gemini finishReason / Responses status）——OpenRouter 的归一策略（5 值归一 + native 透传）。`pause_turn`、`model_context_window_exceeded` 这类被折成 `stop`/`length` 的原生语义不再不可分辨，客户端与排障都有据可依。

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] 三转换协议的非流式响应 choice 含 `native_finish_reason`（上游未给时省略该字段而非 null 猜测）
- [ ] 流式终止 chunk 同样携带；OpenAI Compat 直通路径透传上游已有字段（不重复注入）
- [ ] A5 的语义丢失一并缓解：`pause_turn`/`model_context_window_exceeded` 的原生值可见（归一映射表本身维持现状）
- [ ] 透传值经过枚举校验/截断，不把上游任意字符串透给客户端
