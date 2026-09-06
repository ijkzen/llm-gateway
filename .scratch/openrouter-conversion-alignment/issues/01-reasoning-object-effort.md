# 01: 请求侧识别 OpenRouter `reasoning.effort` 对象参数

**Parent**: `.scratch/protocol-conversion-audit/OPENROUTER-COMPARISON.md` O1

**What to build:** OpenRouter 生态客户端按主形态发 `"reasoning":{"effort":"high"}` 请求时，网关的三个转换协议出口（Anthropic / Gemini / Responses）都能正确开启思考，与现有顶层 `reasoning_effort` 简写行为完全等价；OpenRouter 文档明确 `reasoning_effort` 只是 `reasoning.effort` 的 shorthand，本网关目前只认后者，导致该类客户端思考静默失效（请求成功但不思考，用户难以归因）。

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] 客户端发 `reasoning:{effort}` 打 Anthropic 成员 → 发出 thinking 参数（预算换算与现有 `reasoning_effort` 同口径）
- [ ] 同样请求打 Gemini 成员 → thinkingConfig 生效；打 Responses 成员 → `reasoning.effort` 生效
- [ ] `reasoning` 与 `reasoning_effort` 同时出现时的优先级/冲突规则有明确定义并测试覆盖（建议：不一致时取更明确的一方并告警）
- [ ] OpenAI Compat 直通路径对 `reasoning` 对象的处理策略有明确定义（原样透传或剥离）并测试覆盖
- [ ] 旧参数 `reasoning_effort` 行为回归不变；`include_reasoning` legacy 参数不做（保持忽略）
