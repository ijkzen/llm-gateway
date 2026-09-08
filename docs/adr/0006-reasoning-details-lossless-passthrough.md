# 0006 — 思考块无损透传（reasoning_details，OpenRouter 兼容载体）

## Status

accepted

## Context

上游推理输出携带厂商签名的思考载体：Anthropic `thinking`+`signature` / `redacted_thinking`、Responses reasoning item 的 `encrypted_content`、Gemini functionCall 的 `thoughtSignature`。网关转换到 OpenAI Compat 时这些载体过去被丢弃：客户端拿不到思考内容，且工具轮无法把加密签名原样回传——Anthropic 侧只能整体禁用 thinking（对话双重降智），Responses 侧推理模型工具轮不回传 reasoning item 会 400。调研定论（2026-09-06，见 .scratch/openrouter-conversion-alignment/ 与 CONTEXT 关联词条）：客户端侧回传是唯一可行路径，OpenRouter `reasoning_details` 是跨客户端事实标准；加密载体无法跨厂商互转，服务端维护思考状态在 LB failover 换厂商后必然错位。

## Decision

1. 载体统一为 OpenRouter 兼容 `reasoning_details`：非流式装 `message`、流式装 `delta`，条目为 `reasoning.text`（明文）/ `reasoning.encrypted`（密文）+ `format` 标记来源：`anthropic-claude-v1` / `openai-responses-v1` / `google-gemini-v1`。
2. 响应方向三协议捕获并装入该载体：Anthropic thinking+signature / redacted_thinking、Responses reasoning item（请求侧恒带 `include: ["reasoning.encrypted_content"]`）、Gemini thoughtSignature。捕获/装配/校验 helper 收在 `src/proxy/convert` 公共层（`attach_reasoning_details` / `reasoning_encrypted_detail` / `valid_reasoning_details`）。
3. 请求方向：客户端把 details 原样回传在 assistant 消息上，网关校验后按 `format` 注入对应上游载体（Anthropic 块前置 thinking、Responses item 紧贴 function_call、Gemini 签名按下标挂 functionCall part）；`format` 不匹配（含 failover 换厂商）debug 丢弃，不误注。OpenAI Compat 直通上游在请求侧剥离 `reasoning_details`（内部载体不外泄），存量 `reasoning_content` 归一行为不变。
4. Anthropic 工具轮策略修正：工具轮有块回传则保留 thinking（不再整体禁用），仅最后 tool 轮无块回传时才禁用。

## Consequences

- 加密签名思考载体不透明搬运，多轮工具对话思考全程无损、无 400 与降智（四协议端到端往返集成测试锁定）。
- 只认 `reasoning_content` 的存量客户端不受影响（非 OpenRouter 风格路径输出不变）。
- 跨厂商 failover 安全：格式不匹配即丢弃，不会把 A 厂商签名注给 B 厂商。
- 依赖客户端回传——不走网关回传的直连/旁路场景不在本决策保护范围（此类「流量未全走网关」的信任边界问题见 ADR-0008 与 CONTEXT「用量预估」）。
