# 02: 转发与测速按模型协议生效

**What to build:** `/v1` 转发与模型测速在决定某成员用哪个上游协议时，先看该成员指向的模型是否单独指定了协议——指定了就用它，没指定就回落供应商协议。这样供应商协议为 Anthropic、但某模型单独指定为 OpenAI Responses 时，转发请求自动按 Responses 形态（URL + body）打给上游，测速同规则；未指定协议的模型行为与现状完全一致（回归）。

**Blocked by:** 01（模型协议字段存储与 CRUD——需字段存在才能读取生效）

**Status:** ready-for-agent

- [ ] 转发 `load_members` 组装成员协议：模型 `protocol_type` 非空 → 用它；为空 → 供应商 `protocol_type`
- [ ] 测速（test_model）组装协议使用与转发同一规则
- [ ] converter 层 / failover / LB 零改动（仍读 `member.protocol`）
- [ ] 转发集成测试：供应商=Anthropic(2) + 模型覆盖=Responses(1) → mock 上游收到 Responses 形态出站请求（URL 路径 + body 结构）；模型未覆盖 → 回落 Anthropic（回归现有）
- [ ] 组装断言：模型 `None` → 供应商协议；模型覆盖值 → 覆盖协议
