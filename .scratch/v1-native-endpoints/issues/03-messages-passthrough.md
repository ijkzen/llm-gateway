# 03: /v1/messages 原生透传端点

**What to build:** Anthropic 原生客户端（如 Claude Code）可指向 /v1/messages 使用 Messages 类型虚拟模型：请求/响应体原样透传（仅改写 model 字段为成员真实模型 ID），流式与非流式均支持，响应流原样中继。鉴权接受 x-api-key 或 Authorization Bearer（同一 api_key 表校验）。请求头全量透传（剥离清单黑名单兜底：协议鉴权头由网关生成上游鉴权头替换、hop-by-hop、host/content-length 等），x-opencode-session 回退沿用。错误响应用 Anthropic type/error 原生格式。usage 从 Anthropic 原生响应解析归一落现有 request 表字段（cache_creation+cache_read→input_cache_tokens，推理 token 计入 output）。failover 沿用现有降级策略（流式首字节前可重试）。本工单同时搭好与 /v1/responses 共用的透传管线。

**Blocked by:** 01（接口类型字段与类型路由）。

**Status:** ready-for-agent

- [ ] POST /v1/messages 只接受 Messages(2) 类型虚拟模型，其他类型按 Anthropic 原生错误格式拒绝
- [ ] x-api-key 与 Bearer 双凭证鉴权，未带凭证按 Anthropic 错误格式 401
- [ ] 请求体仅改写 model；响应（流式/非流式）原样中继
- [ ] 请求头全量透传 + 剥离清单兜底 + 上游协议鉴权头注入
- [ ] Anthropic usage 解析（message_start/message_delta 与非流式 usage）落 request 表（缝 C 纯函数单测）
- [ ] 指标落库：成功/失败均落一行，ttft/tps 口径与现有链路一致
- [ ] 集成测试：mock Anthropic 上游验证透传、鉴权、错误格式、usage 落库、failover（缝 A）
