# 05: Anthropic adaptive thinking + budget 比例换算（A2 定案 + O2）

**Parent**: `.scratch/protocol-conversion-audit/FINDINGS.md` A2、`OPENROUTER-COMPARISON.md` O4+O2

**What to build:** 带 `reasoning_effort` 的请求打 Claude 4.6+ 成员不再 400：代际感知后发 `thinking:{type:"adaptive"}` + `output_config.effort`（effort 直接映射、minimal→low、none 不发 effort——OpenRouter 已验证的映射表）；更早的 budget 模式 Claude 维持 `thinking.enabled + budget_tokens`，且换算口径定案为按 max_tokens 比例（ratio max/xhigh=0.95、high=0.8、medium=0.5、low=0.2、minimal=0.1，clamp [1024, 128000]，OpenRouter 同款）或明确拍板保留现有固定档位。

**Blocked by:** 04（依赖模型代际判定）。

**Status:** ready-for-agent

- [ ] Claude 4.6+ 成员 + reasoning_effort 请求 → adaptive + output_config.effort，不再 400，也不因 failover 烧掉其他成员
- [ ] budget 模式（4.5 及更早）成员行为回归不变（或按换算口径拍板结果更新）
- [ ] effort→budget 换算口径定案（比例 vs 档位）并写入实现与测试
- [ ] thinking 与 temperature/tool_choice 互斥规避逻辑对新形态（adaptive）同样成立
- [ ] 400 错误兜底：无法判定代际时的回退策略有定义（回退现状 enabled 形态）
