# 06: usage 归一补 reasoning token 明细

**What to build:** 客户端（ZCode 会读这个字段做 reasoningTokens 展示）在转换链路也能拿到推理 token 明细。归一结构 `Usage` 增加 reasoning token 字段；Responses 方向从 `output_tokens_details.reasoning_tokens` 提取、Gemini 方向从 `usageMetadata.thoughtsTokenCount` 提取、Anthropic 方向无此口径（不提取）；客户端可见 usage（流式尾块与非流式）在总量口径不变的前提下补充 `completion_tokens_details.reasoning_tokens`。OpenAI 直通路径字节透传本就有明细，不动。背景见 `../FINDINGS.md` P8。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Responses/Gemini 转换链路的响应 usage 带 completion_tokens_details.reasoning_tokens（上游有该数据时）
- [ ] completion_tokens 总量口径不变（仍含推理 token），仅新增明细
- [ ] Anthropic 方向与直通路径行为不变
- [ ] request 表落库字段如需扩展一并迁移（注意迁移版本号从 16 起编的历史坑，落库口径变更要考虑存量行）
- [ ] cargo fmt / clippy -D warnings / cargo test 全绿
