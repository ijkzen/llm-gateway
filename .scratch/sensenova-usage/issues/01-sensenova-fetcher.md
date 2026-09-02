# 01: SenseNova fetcher 全链路（续期 + 轮换写回 + 多池窗口）

**What to build:** 配好 refresh_token 的商汤供应商，详情页用量查询走通完整链路：用 `extra.refresh_token` 向商汤 OAuth 端点续期换取 access_token，立即把轮换后的新 refresh_token 写回该供应商 extra，再调 pool-usage 接口取回各积分池用量，逐池产出 5h/7d 用量窗口（label = 池名）。双 host（推理域与控制台域）都能分发到该 fetcher。为此用量窗口模型需支持可选窗口标签。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] QuotaWindow 支持可选 label（序列化缺省省略，既有厂商/旧数据零影响）
- [x] 续期成功后新 refresh_token 立即写回 provider extra（并发冲突最坏偶发报错，不作废凭据）
- [x] pool-usage 响应逐池产出 FiveHour/Weekly 窗口：字符串数值转数字、秒级时间戳、label=池名、plan=plan.name；不走去重逻辑
- [x] `token.sensenova.cn` 与 `platform.sensenova.cn` 均分发到 SenseNova fetcher
- [x] sk- 密钥或失效 refresh_token 走现有鉴权失败链路（401 → Auth 错误）
- [x] 解析纯函数单测：续期响应、多池 JSON（含字符串数值/秒级时间戳/多池 label）
- [x] 集成测试（本地 mock 重定向）：续期 → 写回 → pool-usage 全链路 + 双 host 分发
- [x] 全量质量门绿（cargo fmt/clippy -D warnings/test + 前端 lint/vitest）
