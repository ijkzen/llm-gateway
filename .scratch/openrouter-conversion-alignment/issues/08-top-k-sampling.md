# 08: `top_k` 采样参数支持（O6e）

**Parent**: `.scratch/protocol-conversion-audit/OPENROUTER-COMPARISON.md` O6e

**What to build:** OpenRouter 入口接受 `top_k` 扩展采样参数并映射到支持的协议上游；本网关对 Anthropic 成员透传 `top_k`、对 Gemini 成员映射 `generationConfig.topK`，OpenAI Compat 直通原样透传——带 `top_k` 的请求不再静默丢参。

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] Anthropic 出站请求带上游合法的 `top_k`（含与 thinking 的互斥约束：thinking 启用时按官方规则处理）
- [ ] Gemini 出站 `generationConfig.topK` 映射正确
- [ ] OpenAI Compat 直通透传 `top_k`
- [ ] Responses 出站策略有明确定义（透传或丢弃，OpenAI Responses 上游是否接受需查证后定）
- [ ] 不支持 `top_k` 的成员静默忽略（与网关「不支持则忽略」的既有原则一致）

## Comments

- 2026-09-06 实施。Responses 出口 top_k 透传依据：OpenRouter openapi ResponsesRequest 明确含 top_k（OPENROUTER-COMPARISON.md 专题 5.1）；OpenAI 兼容上游对未知顶层参数普遍宽容，严格上游拒绝时有 failover 兜底，风险接受。
- 非整数 top_k 被静默忽略（as_i64），与「不支持则忽略」原则一致。
