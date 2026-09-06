# 07: Responses 密文回传断链——先实测 ZCode 透传行为，再定修复

**What to build:** 一张「调研结论 + 按需修复」的工单。背景：ZCode 的 AI SDK 层不解析 `reasoning_details`，网关回传的 Responses `encrypted_content` 可能在 ZCode 侧被丢弃，导致 gpt-5/o 系工具对话后续轮次缺 reasoning item 而有 400 风险（`../FINDINGS.md` P2）。但尚未验证 ZCode 组装历史消息时是否**原样透传未知 message 字段**——若透传，reasoning_details 会随历史回到网关，链其实是通的，本工单只需沉淀结论。第一步：用真实 ZCode + 网关 + Responses 上游构造多轮工具对话，抓包/日志确认 ZCode 回传的 assistant 消息里有没有 reasoning_details。第二步：断了则定修复方案（候选：响应侧双写 AI SDK 可读的载体 / 文档化为不支持组合 / 其他）；通则更新 FINDINGS.md P2 状态为已闭环。

**Blocked by:** None (can start immediately)

**Status:** 已静态定论（2026-09-07）——ZCode 不回传 reasoning_details（响应解析 zod strict 剥离未知字段 + 内部模型无此概念 + 历史回传逐字段重建），断链确认；AI SDK openai-compatible 线上格式无可双写载体，结论为「不支持组合（ZCode + Responses 上游 + 多轮工具）」，等 ZCode 侧支持 reasoning_details 后再议。详见 ../FINDINGS.md P2 补记。

- [ ] 有实测结论：ZCode 多轮工具对话回传的 assistant 消息是否携带 reasoning_details（附抓包/日志证据）
- [ ] 若断链：修复方案落地并有回归测试；若通畅：FINDINGS.md P2 状态更新为实测闭环
- [ ] 结论同步进 `../FINDINGS.md`
