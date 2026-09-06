# 03: Anthropic 非流式 usage 明细对齐 + 缓存口径改为只算 read（O5 + A6 定案）

**Parent**: `.scratch/protocol-conversion-audit/OPENROUTER-COMPARISON.md` O5、FINDINGS A6

**What to build:** 客户端无论打到哪个协议的成员，非流式响应的 usage 都带 `prompt_tokens_details.cached_tokens` 缓存命中明细（当前唯独 Anthropic 非流式缺失）；同时缓存命中口径定案为**只算 cache_read**——Anthropic 的 cache_creation 是写入不是命中，计入后数据面板命中率系统性虚高（OpenRouter 官方口径：`cached_tokens`=读缓存、写缓存单列 `cache_write_tokens`）。

**决策记录**（2026-09-06 拍板）：A6 口径改为只算 read。

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] Anthropic 非流式响应 usage 含 `prompt_tokens_details.cached_tokens`（与 Gemini/Responses/流式路径一致）
- [ ] cache_tokens 口径改为只计 cache_read；cache_creation 不再计入命中率指标（含流式与非流式、request 表落库口径同步）
- [ ] 数据面板命中率统计口径变化有验证：新数据按 read-only 统计
- [ ] 历史数据口径差异已评估并在验收时说明（不要求回填历史 request 行，但命中率曲线跨口径日期的分界需知情）
- [ ] 既有 usage 相关测试全部更新并通过
