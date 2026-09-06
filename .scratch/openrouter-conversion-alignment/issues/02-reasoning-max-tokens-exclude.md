# 02: `reasoning.max_tokens` 直传与 `exclude` 支持

**Parent**: `.scratch/protocol-conversion-audit/OPENROUTER-COMPARISON.md` O1 扩展

**What to build:** 客户端可用 Anthropic 风格的 `reasoning:{max_tokens:N}` 直接指定思考预算（绕过 effort 档位换算），也可用 `reasoning:{exclude:true}` 让模型照常思考但响应不携带思考内容——两者都是 OpenRouter `reasoning` 对象的官方语义。

**Blocked by:** 01（共享 reasoning 对象解析归一层，先落归一入口）。

**Status:** ready-for-agent

- [ ] `reasoning.max_tokens` 打 Anthropic 成员 → budget_tokens 直传（下限 1024、需小于 max_tokens 的官方约束仍成立）；打 Gemini 成员 → thinkingBudget 直传
- [ ] `reasoning.max_tokens` 与 `reasoning.effort` 同传时的冲突规则有定义（OpenRouter 语义：二选一）并测试覆盖
- [ ] `exclude:true` 时：上游照常开启思考，但响应/流式 delta 不输出 reasoning_content 与 reasoning_details（内部指标仍记录）
- [ ] `reasoning.enabled:true` 等价于 medium effort 开启（OpenRouter 语义）行为正确
