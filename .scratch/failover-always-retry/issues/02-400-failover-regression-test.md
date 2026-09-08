# 02: 集成回归测试：上游 400 也降级到下一成员

**What to build:** 全链路集成测试锁住本次事故场景——mock 成员 A 上游返回 400（OpenAI 错误体形态，如 insufficient credits），成员 B 正常成功；断言最终响应 200 且由成员 B 服务、落库含成员 A 的降级失败行（request_id 带 -1 后缀、success=false、fail_reason 含上游错误消息）与成员 B 的成功行。仿照既有 429 降级用例结构。本测试在 01 落地前为红（400 不降级），落地后转绿。

**Blocked by:** 01（成员尝试循环去状态码白名单）

**Status:** ready-for-agent

- [ ] 测试先红（在 01 前代码下运行失败，断言 400 会降级）后绿（01 落地后通过）
- [ ] 断言：HTTP 200、成功行成员 B、降级失败行成员 A 带 -1 后缀且 fail_reason 含上游错误消息
