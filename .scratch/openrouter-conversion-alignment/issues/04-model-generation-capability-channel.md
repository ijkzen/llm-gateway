# 04: 模型代际/能力信息通道（prefactor）

**Parent**: `.scratch/protocol-conversion-audit/OPENROUTER-COMPARISON.md` O3+O4 前置

**What to build:** 协议转换层在编码请求时能拿到「这个模型是哪一代」的判定能力（如 Claude 4.6+ 走 adaptive thinking、Gemini 3 走 thinkingLevel），而不是只靠现有的 `provider_model.reasoning` 布尔。这是 05/06 两张票的共同前置：先打通信息通道，再做各协议的具体映射。

**What to build 的形态（待实施时细化）：** provider_model 扩展能力字段（如 reasoning 代际/枚举）或按模型名的判定规则，二选一或有组合；需覆盖管理界面编辑能力与现有模型刷新流程的兼容。

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] 转换层可查询模型代际/能力信息，接口不依赖各协议实现细节
- [ ] 判定方式（字段 vs 规则）已定案并写入实现
- [ ] 现有供应商/模型的兼容：未标注代际的存量模型行为完全不变（回退现状逻辑）
- [ ] 管理端可查看/编辑该信息（或明确说明为何本期不做编辑）
- [ ] 迁移与存量数据对齐遵循「启动迁移幂等」惯例
