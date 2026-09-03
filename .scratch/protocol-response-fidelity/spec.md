Status: ready-for-agent

# 跨协议下游响应保真修复

## Problem Statement

调用方通过网关的 `/v1/chat/completions` 使用 OpenAI Responses、Gemini 等不同上游时，网关会把上游协议转换为 OpenAI Chat Completions 响应。字段审计发现三类保真问题：

- Responses 兼容上游若不发送增量文本或推理事件、只在最终 item 或完成事件中携带结果，网关会丢失最终内容。
- Gemini 已解析并记录缓存命中 token，但下游调用方看不到 `usage.prompt_tokens_details.cached_tokens`；非流式和流式 `include_usage` 都受影响。
- Responses 流式响应的内容 chunk 使用上游响应 id/model，但尾部 usage chunk 使用网关请求 id/虚拟模型名，导致同一 SSE 流的元信息不一致。

这些问题会使调用方得到空内容、无法正确获取缓存计费信息，或无法可靠关联一条流式响应中的所有 chunk。

## Solution

保持 `/v1/chat/completions` 作为唯一对外兼容入口，在现有协议转换器和代理集成测试 seam 上修复响应保真：

- 为 Responses 流添加最终输出兜底：在已无增量事件时，从最终 item 或完成事件的最终输出中恢复可映射的文本、推理内容和函数调用参数，且避免与已发送的增量内容重复。
- 将 Gemini 的缓存命中量映射为下游 OpenAI-compatible `usage.prompt_tokens_details.cached_tokens`，同时覆盖非流式 completion 和请求 `stream_options.include_usage=true` 的流式尾块。
- 使 Responses 的流式 usage 尾块沿用该流已确定的 completion id 和 model。

## User Stories

1. 作为 OpenAI 兼容客户端调用方，我希望 Responses 上游仅在最终事件提供文本时仍能获得完整 assistant 内容，以免收到空回复。
2. 作为 OpenAI 兼容客户端调用方，我希望 Responses 上游仅在最终事件提供 reasoning 内容时仍能获得网关已支持的 `reasoning_content`，以便保持推理输出体验。
3. 作为工具调用客户端，我希望 Responses 上游未发送 function arguments delta 时仍能收到完整的函数调用参数，以便可靠执行工具。
4. 作为流式客户端，我希望 Responses 完成事件的兜底不重复已经发出的文本、推理或函数参数片段，以便不会重复消费内容。
5. 作为 Gemini 上游的调用方，我希望在非流式 Chat Completions usage 中看到缓存命中 token，以便核对缓存计费和 token 使用。
6. 作为 Gemini 上游的流式调用方，我希望请求 usage 尾块后看到缓存命中 token，以便流式与非流式计费口径一致。
7. 作为不请求流式 usage 的客户端，我不希望 Gemini 流中凭空新增 usage chunk，以便保留 OpenAI `stream_options.include_usage` 语义。
8. 作为 Responses 流式调用方，我希望内容 chunk 与 usage 尾块拥有相同的 completion id 和 model，以便安全地聚合一条 SSE 流。
9. 作为 Anthropic 或 Gemini 以外的协议调用方，我希望本次 Responses 内容兜底与 Gemini 缓存改动不改变既有无关字段，以便升级不引入协议回归。
10. 作为维护者，我希望有协议转换单元测试和端到端代理测试锁定上述行为，以便未来转换器改动不会重新丢字段。

## Implementation Decisions

- 继续使用现有 OpenAI Chat Completions 输出模型；不增加新的 HTTP 路由或数据库迁移。
- Responses 兜底仅处理可映射到当前 Chat Completions 输出的最终 message 文本、reasoning 内容、function call 及参数；不尝试透传原生 web search、file search、computer、audio、conversation 等没有当前等价物的 Responses item。
- 兜底必须基于转换器已发出的内容追踪状态，确保最终事件只补齐缺少部分，不能重新发送已有 delta。
- Gemini 继续使用现有归一化 `Usage.cache_tokens`；在 Gemini 面向客户端的 usage 序列化中，当值大于零时追加 `prompt_tokens_details.cached_tokens`。
- Gemini 的普通非流式转换和 `include_usage` 尾块均使用一致的 Gemini usage 序列化；未请求 `include_usage` 时不发送 usage 尾块。
- Responses 流式尾块从 Responses 转换器或已收集的 completion 元数据取得与内容 chunk 相同的 id/model，而不是重新使用网关请求 id 或客户端虚拟模型名。
- `prompt_tokens`、`completion_tokens` 与 `total_tokens` 保持项目现有归一化口径；缓存 token 是输入 token 的子集，不从这些总量中扣除。

## Testing Decisions

- 测试只断言调用方可观察到的 JSON completion 与 SSE frame 行为，不断言私有状态实现。
- 在现有 Responses 转换器单元测试中，使用只有最终 item/完成输出、没有对应 delta 的最小 fixture，验证文本、reasoning 和 function arguments 的兜底及不重复。
- 在现有 Gemini 转换器单元测试中，验证带 `cachedContentTokenCount` 的 non-stream usage 以及 usage chunk 序列化。
- 在现有本地 mock 上游代理集成测试中，驱动 `/v1/chat/completions`：验证 Responses 非流式和流式最终内容兜底、Responses stream usage chunk id/model 一致、Gemini non-stream 缓存字段，以及 Gemini stream + `stream_options.include_usage` 缓存字段。
- 沿用当前协议转换集成测试的 mock server、认证和请求指标等待辅助工具，避免引入新的测试 harness。

## Out of Scope

- Anthropic `cache_read_input_tokens` 与 `cache_creation_input_tokens` 对外映射到 OpenAI cached token 的语义决策。
- Gemini `logprobs` 请求/响应支持。
- Responses、Anthropic 或 Gemini 原生 server tool、citation、grounding、safety detail、音视频/图像输出和多候选响应的完整 OpenAI-compatible 扩展。
- 修改请求指标数据库口径、历史数据或管理后台展示。
- 更改 `/v1/chat/completions` 之外的 API。

## Further Notes

- 当前 worktree 已包含一个未提交的前序修复：Responses 的 `cached_tokens` 已在下游非流式和流式 usage 输出中透传。该修复是本规格的既有前提，实施时应保留且不回退。
- Gemini 缓存字段的下游形状应与现有 Responses 缓存字段一致：`usage.prompt_tokens_details.cached_tokens`。
- 本规格不要求 Git 提交；提交前须单独取得用户确认。
