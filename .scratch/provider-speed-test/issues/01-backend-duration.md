# 01: 后端测速接口返回请求耗时

**What to build:** 现有「测试模型」接口（`POST /api/providers/{provider_id}/models/{model_id}/test`）在测速成功时，除返回成功标记外还返回本次请求的耗时（毫秒）。失败路径行为不变（仍返回人类可读错误）。耗时口径沿用该路径已计算并落库的 `output_tokens_time`（上游响应开始到读完，TTFT 之后的处理 + 传输耗时）。前端由此能展示真实的请求耗时。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 成功测速响应包含数值型耗时字段（如 `data.duration_ms`），数值非负
- [ ] 失败路径行为不变（HTTP 502 + 人类可读错误信息）
- [ ] 集成测试：mock 上游成功时断言响应 `data` 含耗时字段；失败用例保持绿

---

注：这是对既有「测试」功能的扩展，并非新端点。`test_model` 现已在内部算好耗时、仅缺返回一环。前端唯一现役调用方（模型详情测试按钮）忽略返回值即可，行为不受影响。
