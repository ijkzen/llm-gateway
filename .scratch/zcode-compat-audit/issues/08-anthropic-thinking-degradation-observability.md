# 08: Anthropic 工具轮降智可观测化

**What to build:** ZCode 类客户端（丢弃 reasoning_details、无法回传 Anthropic thinking 签名块）触发 `drop_thinking_without_history_blocks` 丢弃 thinking 参数时，这一「推理质量静默降级」从一条 warn 日志升级为可感知信号：在响应中加入客户端可识别的标记（如响应头或 message 元字段，具体形态实现时定），并在 request 表落库记录中体现（便于数据面板/排障反查）。不追求根治（根治依赖 ZCode 支持 reasoning_details），只让降级可见。背景见 `../FINDINGS.md` P3。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] thinking 被丢弃的请求在响应或落库记录中有机器可辨的降级标记
- [ ] 未触发降级的请求无任何行为变化
- [ ] 标记口径写入文档（AGENTS.md 或 FINDINGS.md）
- [ ] cargo fmt / clippy -D warnings / cargo test 全绿
