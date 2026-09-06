# 02: Anthropic tool_result 空内容兜底

**What to build:** 客户端工具输出为空字符串（shell 命令无输出、文件为空等常见场景）时，经网关发往 Anthropic 上游的 `tool_result` 内容不再触发上游「text content blocks must be non-empty」400。做法与现有 user 空消息兜底对齐：空内容替换为占位文本（参照 LiteLLM 惯例用 `"."` 或与 user 侧一致的 `" "`）。背景见 `../FINDINGS.md` P9。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] tool 消息 content 为空字符串/空数组时，Anthropic 请求体中 tool_result.content 为占位文本而非空串
- [ ] 非空内容行为不变
- [ ] 补单元测试覆盖空串与空数组两种输入
- [ ] cargo fmt / clippy -D warnings / cargo test 全绿
