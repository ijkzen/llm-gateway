# 05: Responses 上游 effort 非法值钳制

**What to build:** 客户端下发 ZCode 风格的扩展档位（`max`，以及将来可能出现的其他非 OpenAI 枚举值）时，发往官方 OpenAI Responses 上游的 `reasoning.effort` 不再因非法枚举被 400。做法：Responses 方向对 effort 做白名单钳制/映射（OpenAI 合法枚举为 none/minimal/low/medium/high/xhigh），`max` 映射到 `xhigh`（没有 xhigh 支持预期时降 high），未知值映射到最近档并打 debug 日志。注意只影响 Responses 方向：Anthropic/Gemini 走预算档位表（max→16384）不受影响，OpenAI 直通维持字节透传。背景见 `../FINDINGS.md` P6。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] `reasoning_effort:"max"` 经 Responses 方向发出的 effort 是上游合法枚举值
- [ ] 合法枚举内的档位原样透传不被改写
- [ ] 未知档位有兜底映射且留日志
- [ ] 补单元测试覆盖 max/未知值/合法值三类输入
- [ ] cargo fmt / clippy -D warnings / cargo test 全绿
