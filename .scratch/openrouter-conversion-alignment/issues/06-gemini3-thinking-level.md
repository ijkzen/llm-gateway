# 06: Gemini 3 effort 映射 thinkingLevel（O3）

**Parent**: `.scratch/protocol-conversion-audit/OPENROUTER-COMPARISON.md` O3

**What to build:** 客户端带 `reasoning_effort` 的请求打 Gemini 3 成员时，effort 直接映射上游 `thinkingConfig.thinkingLevel`（minimal/low/medium/high，xhigh 降级 high，不支持档位映射最近档——OpenRouter 同款规则）；Gemini 2.5 系成员维持现有 `thinkingBudget + includeThoughts` 行为不变。

**Blocked by:** 04（依赖模型代际判定区分 Gemini 3 与 2.5）。

**Status:** ready-for-agent

- [ ] Gemini 3 成员 + reasoning_effort → thinkingLevel 正确映射，含档位降级规则
- [ ] Gemini 2.5 系成员行为回归不变（thinkingBudget 路径测试通过）
- [ ] 02 的 `reasoning.max_tokens` 若已合并，Gemini 3 上仍走 thinkingBudget 直传通道（OpenRouter 语义）
- [ ] 无法判定代际时回退现状 thinkingBudget（与 04 的回退约定一致）
