# 05 · proxy 协议转换审查

Type: task
Status: open
Blocked by: 01

## Question

对 proxy 协议转换域做全量审查：`convert/` 全目录（openai/responses/anthropic/gemini × request/response/stream/images + 思考块载体推理 details 搬运 + 域内测试）。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：跨协议字段映射遗漏、思考块 format 匹配/注入、工具轮回传、畸形上游事件处理、非流式全缓冲路径（历次审计已修项不复查）；
- 实现简洁：四协议转换的重复结构与可收敛点（对照 nyro/LiteLLM 参照系）；
- 测试覆盖：mock 回归之外缺什么（字段级等价、边界畸形输入）；
- 模块间调用：Converter 门面与 relay 泵、原生透传旁路的关系是否合适。

产出 `.scratch/code-quality-map-2026-09-09/findings/05-proxy-protocol-conversion.md`，Answer 给摘要与需拍板问题。
