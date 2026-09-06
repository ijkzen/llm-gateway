# 04: chat_reasoning 补认 thinking:{type} 与 enable_thinking 形态

**What to build:** ZCode 对 deepseek 家族、阿里百炼/qwen 家族、以及一切它不认识的模型（默认发 `thinking:{"type":"enabled"}` 二档开关）所发的思考参数，在 Anthropic/Responses/Gemini 转换方向不再被静默忽略。归一层在三态模型（工单 03）上补认两种形态：`thinking.type=="enabled"` 归一为开启（档位取 high 或 medium，参照既有档位表语义）、`"disabled"` 归一为明确关闭；`enable_thinking:true/false` 同理。OpenAI 直通路径维持字节透传不动。背景见 `../FINDINGS.md` P7。

**Blocked by:** 03: chat_reasoning 三态化——区分「未指定」与「明确关闭」

**Status:** ready-for-agent

- [ ] `thinking:{"type":"enabled"}` 在 Anthropic/Gemini/Responses 三方向产生与 reasoning_effort 开启档等价的请求
- [ ] `thinking:{"type":"disabled"}` 与 `enable_thinking:false` 在三方向产生明确关闭语义
- [ ] `enable_thinking:true` 同上开启档
- [ ] 与 reasoning/reasoning_effort 同时出现时的优先级有明确规则并有测试覆盖
- [ ] 端到端验证：虚拟模型用非家族名命名时，ZCode「思考：开」对 Anthropic/Gemini 上游真实生效
- [ ] cargo fmt / clippy -D warnings / cargo test 全绿
